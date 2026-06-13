//! 轻量视觉检测
//!
//! 第一版先提供纯 CPU 简单算法：
//! - ROI 灰度均值 + 磁滞阈值判断灯光
//! - 低分辨率帧差判断画面运动，作为“疑似有人”的轻量信号
//!
//! 注意：帧差只能作为快速任务识别 / VLM 复核触发信号，不能替代正式人形检测。

use crate::pipeline::decoder::DecodedFrame;
use crate::pipeline::PipelineConfig;
use crate::store::{RoiConfig, RoiRect, SceneState};
use std::time::Instant;

const MOTION_WIDTH: u32 = 160;
const MOTION_HEIGHT: u32 = 120;
const MOTION_PIXEL_THRESHOLD: u8 = 20;
const MOTION_AREA_THRESHOLD: f32 = 0.03;
const EMA_ALPHA: f32 = 0.3;

#[derive(Debug, Clone)]
pub struct Detection {
    pub kind: String,    // "motion" | "person" | "vehicle" | ...
    pub confidence: f32, // 0..1
    pub bbox: [f32; 4],  // [x, y, w, h] 归一化坐标
    pub ts: i64,
}

pub struct Detector {
    config: PipelineConfig,
    /// 上一帧（用于帧差法）
    prev: Option<Vec<u8>>,
    prev_low_res: Option<Vec<u8>>,
    light_state: bool,
    brightness_ema: Option<f32>,
    motion_ema: f32,
    frame_counter: u64,
}

#[derive(Debug, Clone)]
pub struct SimpleSceneResult {
    pub scene: SceneState,
    pub light_brightness: f32,
    pub motion_score: f32,
    pub process_ms: f32,
}

impl Detector {
    pub fn new(config: PipelineConfig) -> Self {
        Self {
            config,
            prev: None,
            prev_low_res: None,
            light_state: false,
            brightness_ema: None,
            motion_ema: 0.0,
            frame_counter: 0,
        }
    }

    pub fn analyze_scene(
        &mut self,
        frame: &DecodedFrame,
        roi_config: Option<&RoiConfig>,
    ) -> SimpleSceneResult {
        let started = Instant::now();
        let brightness = average_light_brightness(frame, roi_config);
        let smoothed_brightness = match self.brightness_ema {
            Some(prev) => prev * (1.0 - EMA_ALPHA) + brightness * EMA_ALPHA,
            None => brightness,
        };
        self.brightness_ema = Some(smoothed_brightness);

        let (on_threshold, off_threshold) = roi_config
            .map(|cfg| (cfg.light_on_threshold, cfg.light_off_threshold))
            .unwrap_or((0.70, 0.45));
        let brightness_norm = (smoothed_brightness / 255.0).clamp(0.0, 1.0);
        if brightness_norm >= on_threshold {
            self.light_state = true;
        } else if brightness_norm <= off_threshold {
            self.light_state = false;
        }

        let motion_raw = self.motion_score(frame);
        self.motion_ema = self.motion_ema * (1.0 - EMA_ALPHA) + motion_raw * EMA_ALPHA;
        let person = self.motion_ema >= MOTION_AREA_THRESHOLD;
        let confidence = light_confidence(
            brightness_norm,
            self.light_state,
            on_threshold,
            off_threshold,
        )
        .max(motion_confidence(self.motion_ema));

        SimpleSceneResult {
            scene: SceneState {
                person,
                light: self.light_state,
                frame_seq: self.frame_counter,
                confidence,
            },
            light_brightness: smoothed_brightness,
            motion_score: self.motion_ema,
            process_ms: started.elapsed().as_secs_f32() * 1000.0,
        }
    }

    pub fn detect(&mut self, frame: &DecodedFrame) -> Vec<Detection> {
        // 跳帧
        if self.config.frame_skip > 1 && self.frame_counter % self.config.frame_skip as u64 != 0 {
            self.frame_counter += 1;
            return vec![];
        }
        self.frame_counter += 1;

        // 帧差法占位：朴素像素差，后续替换成 MOG2 / GMM
        let mut dets = vec![];
        if self.config.motion_detection {
            if let Some(prev) = &self.prev {
                if frame.data.len() == prev.len() {
                    let diff: u64 = frame
                        .data
                        .iter()
                        .zip(prev.iter())
                        .map(|(a, b)| (*a as i16 - *b as i16).abs() as u64)
                        .sum();
                    let avg = diff / (frame.data.len().max(1) as u64);
                    if avg > 12 {
                        dets.push(Detection {
                            kind: "motion".into(),
                            confidence: (avg as f32 / 64.0).min(1.0),
                            bbox: [0.0, 0.0, 1.0, 1.0],
                            ts: frame.pts_ms,
                        });
                    }
                }
            }
            self.prev = Some(frame.data.clone());
        }
        dets
    }

    fn motion_score(&mut self, frame: &DecodedFrame) -> f32 {
        let low = downsample_gray(frame, MOTION_WIDTH, MOTION_HEIGHT);
        let score = if let Some(prev) = &self.prev_low_res {
            if prev.len() == low.len() && !low.is_empty() {
                let changed = low
                    .iter()
                    .zip(prev.iter())
                    .filter(|(a, b)| a.abs_diff(**b) > MOTION_PIXEL_THRESHOLD)
                    .count();
                changed as f32 / low.len() as f32
            } else {
                0.0
            }
        } else {
            0.0
        };
        self.prev_low_res = Some(low);
        score
    }
}

fn average_light_brightness(frame: &DecodedFrame, roi_config: Option<&RoiConfig>) -> f32 {
    let rois = roi_config
        .map(|cfg| cfg.light_rois.as_slice())
        .filter(|items| !items.is_empty());
    if let Some(rois) = rois {
        let mut total = 0.0;
        let mut count = 0usize;
        for roi in rois {
            if let Some((sum, pixels)) = roi_sum(frame, roi) {
                total += sum as f32 / pixels as f32;
                count += 1;
            }
        }
        if count > 0 {
            return total / count as f32;
        }
    }
    if frame.data.is_empty() {
        return 0.0;
    }
    frame.data.iter().map(|v| *v as u64).sum::<u64>() as f32 / frame.data.len() as f32
}

fn roi_sum(frame: &DecodedFrame, roi: &RoiRect) -> Option<(u64, usize)> {
    let (x1, y1, x2, y2) = roi_bounds(frame.width, frame.height, roi)?;
    let width = frame.width as usize;
    let mut sum = 0u64;
    let mut pixels = 0usize;
    for y in y1..y2 {
        let row = y * width;
        for x in x1..x2 {
            if let Some(value) = frame.data.get(row + x) {
                sum += *value as u64;
                pixels += 1;
            }
        }
    }
    (pixels > 0).then_some((sum, pixels))
}

fn roi_bounds(width: u32, height: u32, roi: &RoiRect) -> Option<(usize, usize, usize, usize)> {
    if width == 0 || height == 0 {
        return None;
    }
    let x1 = (roi.x.clamp(0.0, 1.0) * width as f32).floor() as usize;
    let y1 = (roi.y.clamp(0.0, 1.0) * height as f32).floor() as usize;
    let x2 = ((roi.x + roi.w).clamp(0.0, 1.0) * width as f32).ceil() as usize;
    let y2 = ((roi.y + roi.h).clamp(0.0, 1.0) * height as f32).ceil() as usize;
    (x2 > x1 && y2 > y1).then_some((x1, y1, x2, y2))
}

fn downsample_gray(frame: &DecodedFrame, target_w: u32, target_h: u32) -> Vec<u8> {
    if frame.width == 0 || frame.height == 0 || frame.data.is_empty() {
        return vec![];
    }
    let out_w = target_w.min(frame.width).max(1);
    let out_h = target_h.min(frame.height).max(1);
    let mut out = Vec::with_capacity((out_w * out_h) as usize);
    for y in 0..out_h {
        let src_y = (y as u64 * frame.height as u64 / out_h as u64) as usize;
        for x in 0..out_w {
            let src_x = (x as u64 * frame.width as u64 / out_w as u64) as usize;
            let idx = src_y * frame.width as usize + src_x;
            out.push(*frame.data.get(idx).unwrap_or(&0));
        }
    }
    out
}

fn light_confidence(value: f32, light: bool, on_threshold: f32, off_threshold: f32) -> f32 {
    let distance = if light {
        (value - off_threshold).max(0.0)
    } else {
        (on_threshold - value).max(0.0)
    };
    let span = (on_threshold - off_threshold).abs().max(0.01);
    (distance / span).clamp(0.0, 1.0)
}

fn motion_confidence(score: f32) -> f32 {
    (score / MOTION_AREA_THRESHOLD).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32, value: u8) -> DecodedFrame {
        DecodedFrame {
            width,
            height,
            pts_ms: 0,
            data: vec![value; (width * height) as usize],
        }
    }

    #[test]
    fn light_detector_uses_hysteresis() {
        let mut detector = Detector::new(PipelineConfig::default());
        let off = detector.analyze_scene(&frame(16, 16, 0), None);
        assert!(!off.scene.light);

        let mut on = detector.analyze_scene(&frame(16, 16, 255), None);
        for _ in 0..3 {
            on = detector.analyze_scene(&frame(16, 16, 255), None);
        }
        assert!(on.scene.light);

        let middle = detector.analyze_scene(&frame(16, 16, 140), None);
        assert!(middle.scene.light);

        for _ in 0..8 {
            detector.analyze_scene(&frame(16, 16, 0), None);
        }
        let recovered = detector.analyze_scene(&frame(16, 16, 0), None);
        assert!(!recovered.scene.light);
    }

    #[test]
    fn light_detector_reads_roi() {
        let mut data = vec![10u8; 100 * 100];
        for y in 25..75 {
            for x in 25..75 {
                data[y * 100 + x] = 240;
            }
        }
        let frame = DecodedFrame {
            width: 100,
            height: 100,
            pts_ms: 0,
            data,
        };
        let mut cfg = RoiConfig::new("src-test".into());
        cfg.light_rois.push(RoiRect {
            id: "roi-1".into(),
            label: "lamp".into(),
            x: 0.25,
            y: 0.25,
            w: 0.5,
            h: 0.5,
        });
        let mut detector = Detector::new(PipelineConfig::default());
        let result = detector.analyze_scene(&frame, Some(&cfg));
        assert!(result.scene.light);
        assert!(result.light_brightness > 200.0);
    }

    #[test]
    fn motion_detector_marks_large_changes() {
        let mut detector = Detector::new(PipelineConfig::default());
        let first = detector.analyze_scene(&frame(64, 48, 0), None);
        assert!(!first.scene.person);
        let second = detector.analyze_scene(&frame(64, 48, 255), None);
        assert!(second.scene.person);
        assert!(second.motion_score > MOTION_AREA_THRESHOLD);
    }
}
