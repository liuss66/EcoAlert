//! 轻量视觉检测
//!
//! 第一版先提供纯 CPU 简单算法：
//! - RGB 彩色程度判断开灯 / 红外黑白关灯
//! - 低分辨率帧差 + 连通域过滤判断画面运动，只作为人员代理信号
//!
//! 注意：帧差只能作为快速任务识别 / VLM 复核触发信号，不能替代正式人形检测。

use crate::pipeline::decoder::DecodedFrame;
use crate::pipeline::PipelineConfig;
use crate::store::{RoiConfig, RoiRect, SceneState};
use std::time::Instant;

const MOTION_WIDTH: u32 = 160;
const MOTION_HEIGHT: u32 = 120;
/// 块均值降采样会平滑像素差值，阈值需要比最近邻采样时更低。
/// 实测块均值后人体移动产生的像素差约 5-25，8 能在噪声(~3)和真实运动之间取得平衡。
const MOTION_PIXEL_THRESHOLD: u8 = 8;
const LEGACY_MOTION_AREA_SCALE: f32 = 0.03;
const DEFAULT_PERSON_AREA_THRESHOLD: f32 = 0.003;
const MIN_MOTION_COMPONENT_AREA: usize = 5;
const MIN_MOTION_COMPONENT_SPAN: usize = 2;
const BLINK_COMPONENT_MAX_AREA: usize = 140;
const BLINK_COMPONENT_MAX_SPAN: usize = 10;
const MICRO_MOTION_BBOX_WEIGHT: f32 = 5.0;
const EMA_ALPHA: f32 = 0.5;
const COLOR_THRESHOLD: f32 = 0.015;
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
    /// 人员检测阈值，表示稳定运动面积比例。旧版 0..1 系数会自动转换。
    person_threshold: f32,
    /// 兼容旧配置；当前正常抽帧始终有 RGB，灯光判断使用 ROI 色彩阈值。
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
        Self::with_thresholds(config, DEFAULT_PERSON_AREA_THRESHOLD, 0.70)
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

    /// 当前没有真人模型，person 是运动代理；阈值越高越严格。
    /// 旧版配置常见为 0.65，需要乘以 0.03 才是实际运动面积阈值。
    fn effective_person_threshold(&self) -> f32 {
        if self.person_threshold > 0.2 {
            (self.person_threshold.clamp(0.05, 1.0) * LEGACY_MOTION_AREA_SCALE).max(0.001)
        } else if (self.person_threshold - 0.020).abs() < 0.0005 {
            DEFAULT_PERSON_AREA_THRESHOLD
        } else {
            self.person_threshold.clamp(0.001, 0.20)
        }
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
        // 正常 ffmpeg 抽帧始终提供 RGB；灰度兜底只保留给单元测试和异常输入。
        let has_color_signal = !frame.rgb.is_empty();
        let color_threshold = color_threshold(roi_config);
        let (brightness_on_threshold, brightness_off_threshold) =
            (self.light_threshold, self.light_threshold * 0.65);
        let brightness_norm = (smoothed_brightness / 255.0).clamp(0.0, 1.0);
        if has_color_signal {
            self.light_state = smoothed_color >= color_threshold;
        } else {
            if brightness_norm >= brightness_on_threshold {
                self.light_state = true;
            } else if brightness_norm <= brightness_off_threshold {
                self.light_state = false;
            }
        }

        let motion_raw = self.motion_score(frame, roi_config);
        self.motion_ema = self.motion_ema * (1.0 - EMA_ALPHA) + motion_raw * EMA_ALPHA;

        // 周期性诊断日志：帮助定位运动检测问题
        if self.frame_counter % 10 == 2 {
            let eff = self.effective_person_threshold();
            log::info!(
                "[detector#{}] frame={}x{} motion_raw={:.4} ema={:.4} thr={:.4} person={} prev={}",
                self.frame_counter,
                frame.width,
                frame.height,
                motion_raw,
                self.motion_ema,
                eff,
                self.motion_ema >= eff,
                self.prev_low_res.as_ref().map_or(0, |b| b.len()),
            );
        }

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
            color_light_confidence(smoothed_color, self.light_state, color_threshold)
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

    fn motion_score(&mut self, frame: &DecodedFrame, roi_config: Option<&RoiConfig>) -> f32 {
        let low = downsample_gray(frame, MOTION_WIDTH, MOTION_HEIGHT);
        let score = if let Some(prev) = &self.prev_low_res {
            if prev.len() == low.len() && !low.is_empty() {
                let width = MOTION_WIDTH.min(frame.width) as usize;
                let height = MOTION_HEIGHT.min(frame.height) as usize;
                let rois = roi_config
                    .map(|cfg| cfg.person_rois.as_slice())
                    .filter(|items| !items.is_empty());
                filtered_motion_area(prev, &low, width, height, rois)
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

fn filtered_motion_area(
    prev: &[u8],
    current: &[u8],
    width: usize,
    height: usize,
    rois: Option<&[RoiRect]>,
) -> f32 {
    if prev.len() != current.len() || current.is_empty() || width == 0 {
        return 0.0;
    }
    let height = height.min(current.len() / width);
    if height == 0 || width * height > current.len() {
        return 0.0;
    }
    let roi_mask = motion_roi_mask(width, height, rois);
    let roi_pixels = roi_mask.iter().filter(|enabled| **enabled).count().max(1);
    let mut changed = vec![false; current.len()];
    for (idx, (a, b)) in current.iter().zip(prev.iter()).enumerate() {
        changed[idx] =
            roi_mask.get(idx).copied().unwrap_or(false) && a.abs_diff(*b) > MOTION_PIXEL_THRESHOLD;
    }

    let mut visited = vec![false; current.len()];
    let mut score = 0.0f32;
    let mut stack = Vec::new();
    for idx in 0..changed.len() {
        if !changed[idx] || visited[idx] {
            continue;
        }
        visited[idx] = true;
        stack.push(idx);
        let mut area = 0usize;
        let mut min_x = width;
        let mut min_y = height;
        let mut max_x = 0usize;
        let mut max_y = 0usize;
        while let Some(p) = stack.pop() {
            area += 1;
            let x = p % width;
            let y = p / width;
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            let mut neighbors = [None; 4];
            if x > 0 {
                neighbors[0] = Some(p - 1);
            }
            if x + 1 < width {
                neighbors[1] = Some(p + 1);
            }
            if y > 0 {
                neighbors[2] = Some(p - width);
            }
            if y + 1 < height {
                neighbors[3] = Some(p + width);
            }
            for next in neighbors.into_iter().flatten() {
                if changed.get(next).copied().unwrap_or(false) && !visited[next] {
                    visited[next] = true;
                    stack.push(next);
                }
            }
        }
        let span_x = max_x.saturating_sub(min_x) + 1;
        let span_y = max_y.saturating_sub(min_y) + 1;
        let human_like_micro_motion = area >= MIN_MOTION_COMPONENT_AREA
            && span_x >= MIN_MOTION_COMPONENT_SPAN
            && span_y >= MIN_MOTION_COMPONENT_SPAN
            && (span_x >= BLINK_COMPONENT_MAX_SPAN || span_y >= BLINK_COMPONENT_MAX_SPAN);
        let compact_blink = !human_like_micro_motion
            && area <= BLINK_COMPONENT_MAX_AREA
            && span_x <= BLINK_COMPONENT_MAX_SPAN
            && span_y <= BLINK_COMPONENT_MAX_SPAN;
        let larger_motion = area >= BLINK_COMPONENT_MAX_AREA
            && span_x >= MIN_MOTION_COMPONENT_SPAN
            && span_y >= MIN_MOTION_COMPONENT_SPAN;
        if !compact_blink && (human_like_micro_motion || larger_motion) {
            let area_ratio = area as f32 / roi_pixels as f32;
            let bbox_ratio = (span_x * span_y) as f32 / roi_pixels as f32;
            score += area_ratio.max(bbox_ratio * MICRO_MOTION_BBOX_WEIGHT);
        }
    }
    score.clamp(0.0, 1.0)
}

fn motion_roi_mask(width: usize, height: usize, rois: Option<&[RoiRect]>) -> Vec<bool> {
    let mut mask = vec![false; width * height];
    let Some(rois) = rois else {
        mask.fill(true);
        return mask;
    };
    let mut any = false;
    for roi in rois {
        let Some((x1, y1, x2, y2)) = roi_bounds(width as u32, height as u32, roi) else {
            continue;
        };
        for y in y1..y2 {
            let row = y * width;
            for x in x1..x2 {
                if let Some(item) = mask.get_mut(row + x) {
                    *item = true;
                    any = true;
                }
            }
        }
    }
    if !any {
        mask.fill(true);
    }
    mask
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

fn color_threshold(roi_config: Option<&RoiConfig>) -> f32 {
    let Some(cfg) = roi_config else {
        return COLOR_THRESHOLD;
    };
    // 旧版本 ROI 阈值是亮度归一化阈值，常见为 0.70 / 0.45。
    // 彩色分数通常在 0.00-0.15 范围内；检测到旧值时回退默认色彩阈值。
    if cfg.light_threshold > 0.2 {
        COLOR_THRESHOLD
    } else {
        cfg.light_threshold.clamp(0.0, 0.2)
    }
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
    let out_w = target_w.min(frame.width).max(1) as usize;
    let out_h = target_h.min(frame.height).max(1) as usize;
    let src_w = frame.width as usize;
    let src_h = frame.height as usize;
    let mut out = Vec::with_capacity(out_w * out_h);
    // 块均值降采样：对每个目标像素，取源图对应矩形块内所有像素的平均值。
    // 相比最近邻采样，能有效抑制噪声和压缩伪影，保留真实的运动信号。
    for oy in 0..out_h {
        let src_y_start = oy * src_h / out_h;
        let src_y_end = (oy + 1) * src_h / out_h;
        for ox in 0..out_w {
            let src_x_start = ox * src_w / out_w;
            let src_x_end = (ox + 1) * src_w / out_w;
            let mut sum = 0u64;
            let mut count = 0u64;
            for sy in src_y_start..src_y_end {
                let row = sy * src_w;
                for sx in src_x_start..src_x_end {
                    sum += *frame.data.get(row + sx).unwrap_or(&0) as u64;
                    count += 1;
                }
            }
            out.push(if count > 0 { (sum / count) as u8 } else { 0 });
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

fn color_light_confidence(color_score: f32, light: bool, threshold: f32) -> f32 {
    if threshold <= 0.0 {
        return if light { 1.0 } else { 0.0 };
    }
    let distance = if light {
        (color_score - threshold).max(0.0)
    } else {
        (threshold - color_score).max(0.0)
    };
    (distance / threshold).clamp(0.0, 1.0)
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
        assert!(infrared.scene.color_score < COLOR_THRESHOLD);

        let mut color = detector.analyze_scene(&rgb_frame(16, 16, [220, 120, 40]), None);
        for _ in 0..3 {
            color = detector.analyze_scene(&rgb_frame(16, 16, [220, 120, 40]), None);
        }
        assert!(color.scene.light);
        assert!(color.scene.color_score > COLOR_THRESHOLD);

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
        let mut result = detector.analyze_scene(&rgb_frame(16, 16, [185, 183, 181]), None);
        for _ in 0..3 {
            result = detector.analyze_scene(&rgb_frame(16, 16, [185, 183, 181]), None);
        }
        assert!(!result.scene.light);
        assert!(result.scene.color_score < COLOR_THRESHOLD);
    }

    #[test]
    fn light_detector_uses_configured_color_thresholds() {
        let mut cfg = RoiConfig::new("src-test".into());
        cfg.light_threshold = 0.03;
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
        cfg.light_threshold = 0.70;
        let mut detector = Detector::new(PipelineConfig::default());

        let mut color = detector.analyze_scene(&rgb_frame(16, 16, [220, 120, 40]), Some(&cfg));
        for _ in 0..3 {
            color = detector.analyze_scene(&rgb_frame(16, 16, [220, 120, 40]), Some(&cfg));
        }
        assert!(color.scene.light);
        assert!(color.scene.color_score > COLOR_THRESHOLD);
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
        assert!(second.motion_score > DEFAULT_PERSON_AREA_THRESHOLD);
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
    fn person_not_detected_for_small_blinking_device() {
        let mut detector = Detector::new(PipelineConfig::default());
        let base = frame(160, 120, 80);
        detector.analyze_scene(&base, None);

        let mut data = vec![80u8; 160 * 120];
        for y in 20..25 {
            for x in 20..25 {
                data[y * 160 + x] = 180;
            }
        }
        let blink = DecodedFrame {
            width: 160,
            height: 120,
            pts_ms: 0,
            data,
            rgb: vec![],
        };
        let result = detector.analyze_scene(&blink, None);
        assert!(!result.scene.person);
        assert_eq!(result.scene.motion_score, 0.0);
    }

    #[test]
    fn person_detected_for_subtle_desk_motion() {
        let mut detector = Detector::new(PipelineConfig::default());
        let base = frame(160, 120, 80);
        detector.analyze_scene(&base, None);

        let mut data = vec![80u8; 160 * 120];
        for y in 40..44 {
            for x in 40..52 {
                data[y * 160 + x] = 108;
            }
        }
        let moved = DecodedFrame {
            width: 160,
            height: 120,
            pts_ms: 0,
            data,
            rgb: vec![],
        };
        detector.analyze_scene(&moved, None);
        let result = detector.analyze_scene(&base, None);
        assert!(result.scene.person, "工位微动应能触发人员代理");
    }

    #[test]
    fn motion_detector_respects_person_roi() {
        let mut cfg = RoiConfig::new("src-test".into());
        cfg.person_rois.push(RoiRect {
            id: "person-roi".into(),
            label: "desk".into(),
            x: 0.25,
            y: 0.25,
            w: 0.5,
            h: 0.5,
        });

        let mut detector = Detector::new(PipelineConfig::default());
        let base = frame(160, 120, 80);
        detector.analyze_scene(&base, Some(&cfg));

        let mut outside_data = vec![80u8; 160 * 120];
        for y in 0..30 {
            for x in 0..30 {
                outside_data[y * 160 + x] = 130;
            }
        }
        let outside = DecodedFrame {
            width: 160,
            height: 120,
            pts_ms: 0,
            data: outside_data,
            rgb: vec![],
        };
        let outside_result = detector.analyze_scene(&outside, Some(&cfg));
        assert!(!outside_result.scene.person);
        assert_eq!(outside_result.scene.motion_score, 0.0);

        let mut detector = Detector::new(PipelineConfig::default());
        detector.analyze_scene(&base, Some(&cfg));
        let mut inside_data = vec![80u8; 160 * 120];
        for y in 50..56 {
            for x in 60..76 {
                inside_data[y * 160 + x] = 130;
            }
        }
        let inside = DecodedFrame {
            width: 160,
            height: 120,
            pts_ms: 0,
            data: inside_data,
            rgb: vec![],
        };
        let inside_result = detector.analyze_scene(&inside, Some(&cfg));
        assert!(inside_result.scene.person);
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
        let mut detector_low = Detector::with_thresholds(PipelineConfig::default(), 0.003, 0.7);
        // 第一帧建立基线
        detector_low.analyze_scene(&frame(64, 48, 100), None);
        // 第二帧：中等运动（差值 30 > MOTION_PIXEL_THRESHOLD=16，所有像素变化）
        // motion_raw=1.0, motion_ema=0.3
        // direct threshold = 0.003, 0.3 >= 0.003 -> person=true
        let result_low = detector_low.analyze_scene(&frame(64, 48, 130), None);
        assert!(result_low.scene.person, "低阈值下中等运动应触发 person");

        // 高阈值：同样的低运动不触发
        let mut detector_high = Detector::with_thresholds(PipelineConfig::default(), 0.08, 0.7);
        detector_high.analyze_scene(&frame(64, 48, 100), None);
        let mut detector_high2 = Detector::with_thresholds(PipelineConfig::default(), 0.08, 0.7);
        detector_high2.analyze_scene(&frame(64, 48, 100), None);
        let result_high = detector_high2.analyze_scene(&frame(64, 48, 105), None);
        assert!(!result_high.scene.person, "极高阈值+低运动不应触发 person");

        // 验证有效阈值映射关系
        assert!(
            detector_high.effective_person_threshold() > detector_low.effective_person_threshold()
        );
    }
}
