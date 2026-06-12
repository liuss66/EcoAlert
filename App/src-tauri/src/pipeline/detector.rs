//! 目标检测 / 运动侦测（占位）
//!
//! 计划分两步：
//! 1. 先做轻量的运动侦测（背景建模 / 帧差法），用于验证流水线框架
//! 2. 再接入 ONNX Runtime + YOLOv8 / RT-DETR，做目标检测

use crate::pipeline::decoder::DecodedFrame;
use crate::pipeline::PipelineConfig;

#[derive(Debug, Clone)]
pub struct Detection {
    pub kind: String,         // "motion" | "person" | "vehicle" | ...
    pub confidence: f32,      // 0..1
    pub bbox: [f32; 4],       // [x, y, w, h] 归一化坐标
    pub ts: i64,
}

pub struct Detector {
    config: PipelineConfig,
    /// 上一帧（用于帧差法）
    prev: Option<Vec<u8>>,
    frame_counter: u64,
}

impl Detector {
    pub fn new(config: PipelineConfig) -> Self {
        Self { config, prev: None, frame_counter: 0 }
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
                    let diff: u64 = frame.data.iter().zip(prev.iter())
                        .map(|(a, b)| ((*a as i16 - *b as i16).abs() as u64))
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
}
