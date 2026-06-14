//! 轻量视觉检测
//!
//! 第一版先提供纯 CPU 简单算法：
//! - RGB 彩色程度判断开灯 / 红外黑白关灯，ROI 灰度亮度兜底
//! - 低分辨率帧差判断画面运动，只作为任务触发 / 复核信号
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
const COLOR_ON_THRESHOLD: f32 = 0.055;
const COLOR_OFF_THRESHOLD: f32 = 0.025;
const COLOR_DARK_LUMA_CUTOFF: f32 = 24.0;
const COLOR_WEIGHT_LUMA_FLOOR: f32 = 40.0;

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
    color_ema: Option<f32>,
    motion_ema: f32,
    frame_counter: u64,
    /// 人员检测阈值（来自 AlgorithmConfig.person_threshold，0..1）
    person_threshold: f32,
    /// 无 RGB 数据时的亮度兜底阈值（来自 AlgorithmConfig.light_threshold，0..1）
    light_threshold: f32,
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
        Self::with_thresholds(config, 0.65, 0.70)
    }

    pub fn with_thresholds(
        config: PipelineConfig,
        person_threshold: f32,
        light_threshold: f32,
    ) -> Self {
        Self {
            config,
            prev: None,
            prev_low_res: None,
            light_state: false,
            brightness_ema: None,
            color_ema: None,
            motion_ema: 0.0,
            frame_counter: 0,
            person_threshold,
            light_threshold,
        }
    }

    /// 运行时更新阈值，不丢失 EMA / 帧差状态
    pub fn set_thresholds(&mut self, person_threshold: f32, light_threshold: f32) {
        self.person_threshold = person_threshold;
        self.light_threshold = light_threshold;
    }

    /// 将 person_threshold（0..1）映射为运动面积阈值。
    /// 当前没有真人模型，person 只是运动代理；阈值越高越严格。
    fn effective_person_threshold(&self) -> f32 {
        (self.person_threshold.clamp(0.05, 1.0) * MOTION_AREA_THRESHOLD).max(0.001)
    }

    pub fn analyze_scene(
        &mut self,
        frame: &DecodedFrame,
        roi_config: Option<&RoiConfig>,
    ) -> SimpleSceneResult {
        let started = Instant::now();
        self.frame_counter += 1;
        let brightness = average_light_brightness(frame, roi_config);
        let smoothed_brightness = match self.brightness_ema {
            Some(prev) => prev * (1.0 - EMA_ALPHA) + brightness * EMA_ALPHA,
            None => brightness,
        };
        self.brightness_ema = Some(smoothed_brightness);

        let color_score = average_color_score(frame, roi_config);
        let smoothed_color = match self.color_ema {
            Some(prev) => prev * (1.0 - EMA_ALPHA) + color_score * EMA_ALPHA,
            None => color_score,
        };
        self.color_ema = Some(smoothed_color);

        // 优先使用摄像头模式特征：开灯为彩色图像，关灯红外为黑白图像。
        // 无 RGB 数据时回退到 ROI / 全帧亮度阈值。
        let has_color_signal = !frame.rgb.is_empty();
        let (color_on_threshold, color_off_threshold) = color_thresholds(roi_config);
        let (brightness_on_threshold, brightness_off_threshold) =
            (self.light_threshold, self.light_threshold * 0.65);
        let brightness_norm = (smoothed_brightness / 255.0).clamp(0.0, 1.0);
        if has_color_signal {
            if smoothed_color >= color_on_threshold {
                self.light_state = true;
            } else if smoothed_color <= color_off_threshold {
                self.light_state = false;
            }
        } else {
            if brightness_norm >= brightness_on_threshold {
                self.light_state = true;
            } else if brightness_norm <= brightness_off_threshold {
                self.light_state = false;
            }
        }

        let motion_raw = self.motion_score(frame);
        self.motion_ema = self.motion_ema * (1.0 - EMA_ALPHA) + motion_raw * EMA_ALPHA;

        // 基于运动得分的近似人员检测
        let eff_threshold = self.effective_person_threshold();
        let person = eff_threshold > 0.0 && self.motion_ema >= eff_threshold;
        let person_confidence = if eff_threshold > 0.0 {
            if self.motion_ema >= eff_threshold {
                0.5 + 0.5 * ((self.motion_ema - eff_threshold) / eff_threshold).min(1.0)
            } else {
                0.5 * (self.motion_ema / eff_threshold)
            }
        } else {
            0.0
        };

        let light_conf = if has_color_signal {
            color_light_confidence(
                smoothed_color,
                self.light_state,
                color_on_threshold,
                color_off_threshold,
            )
        } else {
            light_confidence(
                brightness_norm,
                self.light_state,
                brightness_on_threshold,
                brightness_off_threshold,
            )
        };

        let process_ms = started.elapsed().as_secs_f32() * 1000.0;
        let reason = if person {
            "simple_motion_proxy"
        } else {
            "simple_no_motion"
        };

        SimpleSceneResult {
            scene: SceneState {
                person,
                light: self.light_state,
                frame_seq: self.frame_counter,
                confidence: light_conf,
                source: "simple".into(),
                person_confidence,
                light_confidence: light_conf,
                reason: Some(format!(
                    "{reason};light_by_{}",
                    if has_color_signal {
                        "color"
                    } else {
                        "brightness"
                    }
                )),
                model_latency_ms: Some(process_ms as u32),
                light_brightness: smoothed_brightness,
                color_score: smoothed_color,
                motion_score: self.motion_ema,
                process_ms,
            },
            light_brightness: smoothed_brightness,
            motion_score: self.motion_ema,
            process_ms,
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

fn average_color_score(frame: &DecodedFrame, roi_config: Option<&RoiConfig>) -> f32 {
    if frame.rgb.len() != frame.data.len().saturating_mul(3) || frame.rgb.is_empty() {
        return 0.0;
    }
    let rois = roi_config
        .map(|cfg| cfg.light_rois.as_slice())
        .filter(|items| !items.is_empty());
    if let Some(rois) = rois {
        let mut total = 0.0;
        let mut count = 0usize;
        for roi in rois {
            if let Some((score, _pixels)) = roi_color_sum(frame, roi) {
                total += score;
                count += 1;
            }
        }
        if count > 0 {
            return total / count as f32;
        }
    }
    color_sum_for_bounds(frame, 0, 0, frame.width as usize, frame.height as usize)
        .map(|(score, _pixels)| score)
        .unwrap_or(0.0)
}

fn color_thresholds(roi_config: Option<&RoiConfig>) -> (f32, f32) {
    let Some(cfg) = roi_config else {
        return (COLOR_ON_THRESHOLD, COLOR_OFF_THRESHOLD);
    };
    // 旧版本 ROI 阈值是亮度归一化阈值，常见为 0.70 / 0.45。
    // 彩色分数通常在 0.00-0.15 范围内；检测到旧值时回退默认色彩阈值。
    if cfg.light_on_threshold > 0.2 || cfg.light_off_threshold > 0.2 {
        return (COLOR_ON_THRESHOLD, COLOR_OFF_THRESHOLD);
    }
    let on = cfg.light_on_threshold.clamp(0.0, 0.2);
    let off = cfg.light_off_threshold.clamp(0.0, on);
    (on, off)
}

fn roi_color_sum(frame: &DecodedFrame, roi: &RoiRect) -> Option<(f32, usize)> {
    let (x1, y1, x2, y2) = roi_bounds(frame.width, frame.height, roi)?;
    color_sum_for_bounds(frame, x1, y1, x2, y2)
}

fn color_sum_for_bounds(
    frame: &DecodedFrame,
    x1: usize,
    y1: usize,
    x2: usize,
    y2: usize,
) -> Option<(f32, usize)> {
    let width = frame.width as usize;
    let mut sum = 0.0f32;
    let mut weight_sum = 0.0f32;
    let mut pixels = 0usize;
    for y in y1..y2 {
        for x in x1..x2 {
            let idx = (y * width + x) * 3;
            let Some(px) = frame.rgb.get(idx..idx + 3) else {
                continue;
            };
            if let Some((score, weight)) = pixel_color_score(px) {
                sum += score * weight;
                weight_sum += weight;
                pixels += 1;
            }
        }
    }
    (weight_sum > 0.0).then_some((sum / weight_sum, pixels.max(1)))
}

fn pixel_color_score(px: &[u8]) -> Option<(f32, f32)> {
    if px.len() < 3 {
        return None;
    }
    let r = px[0] as f32;
    let g = px[1] as f32;
    let b = px[2] as f32;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let luma = (77.0 * r + 150.0 * g + 29.0 * b) / 256.0;
    if luma < COLOR_DARK_LUMA_CUTOFF {
        return None;
    }
    let chroma = (max - min) / max.max(1.0);
    // 暗部压缩噪声和红外近黑区域容易产生微小通道差，按亮度降低权重。
    let weight =
        ((luma - COLOR_WEIGHT_LUMA_FLOOR) / (255.0 - COLOR_WEIGHT_LUMA_FLOOR)).clamp(0.05, 1.0);
    Some((chroma * weight, weight))
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

fn color_light_confidence(
    color_score: f32,
    light: bool,
    on_threshold: f32,
    off_threshold: f32,
) -> f32 {
    let distance = if light {
        (color_score - off_threshold).max(0.0)
    } else {
        (on_threshold - color_score).max(0.0)
    };
    let span = (on_threshold - off_threshold).max(0.01);
    (distance / span).clamp(0.0, 1.0)
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
            rgb: vec![],
        }
    }

    fn rgb_frame(width: u32, height: u32, rgb: [u8; 3]) -> DecodedFrame {
        let mut rgb_data = Vec::with_capacity((width * height * 3) as usize);
        for _ in 0..(width * height) {
            rgb_data.extend_from_slice(&rgb);
        }
        let gray = ((77 * rgb[0] as u32 + 150 * rgb[1] as u32 + 29 * rgb[2] as u32) >> 8) as u8;
        DecodedFrame {
            width,
            height,
            pts_ms: 0,
            data: vec![gray; (width * height) as usize],
            rgb: rgb_data,
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
    fn light_detector_uses_color_mode_switch() {
        let mut detector = Detector::new(PipelineConfig::default());
        let infrared = detector.analyze_scene(&rgb_frame(16, 16, [128, 128, 128]), None);
        assert!(!infrared.scene.light);
        assert!(infrared.scene.color_score < COLOR_OFF_THRESHOLD);

        let mut color = detector.analyze_scene(&rgb_frame(16, 16, [220, 120, 40]), None);
        for _ in 0..3 {
            color = detector.analyze_scene(&rgb_frame(16, 16, [220, 120, 40]), None);
        }
        assert!(color.scene.light);
        assert!(color.scene.color_score > COLOR_ON_THRESHOLD);

        let mut back_to_ir = detector.analyze_scene(&rgb_frame(16, 16, [180, 180, 180]), None);
        for _ in 0..8 {
            back_to_ir = detector.analyze_scene(&rgb_frame(16, 16, [180, 180, 180]), None);
        }
        assert!(!back_to_ir.scene.light);
    }

    #[test]
    fn color_score_ignores_dark_chroma_noise() {
        let mut detector = Detector::new(PipelineConfig::default());
        let noisy_dark = detector.analyze_scene(&rgb_frame(16, 16, [20, 4, 3]), None);
        assert!(!noisy_dark.scene.light);
        assert_eq!(noisy_dark.scene.color_score, 0.0);
    }

    #[test]
    fn bright_near_gray_infrared_stays_off() {
        let mut detector = Detector::new(PipelineConfig::default());
        let mut result = detector.analyze_scene(&rgb_frame(16, 16, [185, 182, 180]), None);
        for _ in 0..3 {
            result = detector.analyze_scene(&rgb_frame(16, 16, [185, 182, 180]), None);
        }
        assert!(!result.scene.light);
        assert!(result.scene.color_score < COLOR_OFF_THRESHOLD);
    }

    #[test]
    fn light_detector_uses_configured_color_thresholds() {
        let mut cfg = RoiConfig::new("src-test".into());
        cfg.light_on_threshold = 0.03;
        cfg.light_off_threshold = 0.015;
        let mut detector = Detector::new(PipelineConfig::default());

        let muted_color = detector.analyze_scene(&rgb_frame(16, 16, [160, 120, 110]), Some(&cfg));
        assert!(muted_color.scene.light);
        assert!(muted_color
            .scene
            .reason
            .as_deref()
            .unwrap_or("")
            .contains("light_by_color"));
        assert!(muted_color.scene.light_confidence > 0.5);
    }

    #[test]
    fn legacy_roi_brightness_thresholds_do_not_break_color_mode() {
        let mut cfg = RoiConfig::new("src-test".into());
        cfg.light_on_threshold = 0.70;
        cfg.light_off_threshold = 0.45;
        let mut detector = Detector::new(PipelineConfig::default());

        let mut color = detector.analyze_scene(&rgb_frame(16, 16, [220, 120, 40]), Some(&cfg));
        for _ in 0..3 {
            color = detector.analyze_scene(&rgb_frame(16, 16, [220, 120, 40]), Some(&cfg));
        }
        assert!(color.scene.light);
        assert!(color.scene.color_score > COLOR_ON_THRESHOLD);
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
            rgb: vec![],
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
    fn motion_detector_uses_motion_as_person_proxy() {
        let mut detector = Detector::new(PipelineConfig::default());
        // 第一帧：无历史，person 应为 false
        let first = detector.analyze_scene(&frame(64, 48, 0), None);
        assert!(!first.scene.person);
        // 第二帧：巨大帧差（0 -> 255），motion_score 极高，应判定 person = true
        let second = detector.analyze_scene(&frame(64, 48, 255), None);
        assert!(second.scene.person, "大帧差应触发 person 检测");
        assert!(second.scene.person_confidence > 0.5);
        assert!(second.motion_score > MOTION_AREA_THRESHOLD);
    }

    #[test]
    fn person_not_detected_with_low_motion() {
        let mut detector = Detector::new(PipelineConfig::default());
        // 连续送相似帧（低运动），person 应为 false
        for _ in 0..5 {
            let result = detector.analyze_scene(&frame(64, 48, 100), None);
            assert!(!result.scene.person, "低运动场景不应触发 person");
            assert!(result.scene.person_confidence < 0.5);
        }
    }

    #[test]
    fn scene_state_extended_fields_populated() {
        let mut detector = Detector::new(PipelineConfig::default());
        let result = detector.analyze_scene(&frame(64, 48, 128), None);
        assert_eq!(result.scene.source, "simple");
        assert!(result.scene.model_latency_ms.is_some());
        assert!(result.scene.light_confidence >= 0.0 && result.scene.light_confidence <= 1.0);
        assert!(result.scene.person_confidence >= 0.0 && result.scene.person_confidence <= 1.0);
        assert!(result.scene.reason.is_some());
        assert!(result.scene.frame_seq > 0);
        assert!(result.scene.light_brightness >= 0.0);
        assert!(result.scene.motion_score >= 0.0);
        assert!(result.scene.process_ms >= 0.0);
        assert!(result.scene.color_score >= 0.0);
    }

    #[test]
    fn threshold_update_affects_person_decision() {
        // 低阈值：更容易触发 person 运动代理
        let mut detector_low = Detector::with_thresholds(PipelineConfig::default(), 0.2, 0.7);
        // 第一帧建立基线
        detector_low.analyze_scene(&frame(64, 48, 100), None);
        // 第二帧：中等运动（差值 30 > MOTION_PIXEL_THRESHOLD=20，所有像素变化）
        // motion_raw=1.0, motion_ema=0.3
        // effective = 0.2 * 0.03 = 0.006, 0.3 >= 0.006 -> person=true
        let result_low = detector_low.analyze_scene(&frame(64, 48, 130), None);
        assert!(result_low.scene.person, "低阈值下中等运动应触发 person");

        // 高阈值：同样的低运动不触发
        let mut detector_high = Detector::with_thresholds(PipelineConfig::default(), 0.999, 0.7);
        detector_high.analyze_scene(&frame(64, 48, 100), None);
        let mut detector_high2 = Detector::with_thresholds(PipelineConfig::default(), 0.999, 0.7);
        detector_high2.analyze_scene(&frame(64, 48, 100), None);
        let result_high = detector_high2.analyze_scene(&frame(64, 48, 105), None);
        assert!(!result_high.scene.person, "极高阈值+低运动不应触发 person");

        // 验证有效阈值映射关系
        assert!(
            detector_high.effective_person_threshold() > detector_low.effective_person_threshold()
        );
    }
}
