//! 视频处理流水线
//!
//! 负责：从解码后的帧 → 检测 / 分析 → 告警规则 → 输出事件
//!
//! 每个视频源在 App 启动时由 `stream::registry` 注册一个 Pipeline，
//! 跑在自己的 tokio task 上，接收 stream 推送的帧（`FramePacket`），
//! 串行走 decoder → detector → analyzer → alerts 四步。
//!
//! 帧数据不在前端展示（太大），Pipeline 只输出轻量结果
//! （状态、计数、告警），通过 Tauri event 推给 webui。

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

pub mod alerts;
pub mod analyzer;
pub mod channel_auth;
pub mod decoder;
pub mod detector;
pub mod notifier;
pub mod oauth_server;
pub mod scheduler;
pub mod vlm;
pub mod yolo_detector;

use crate::stream::FramePacket;

/// 流水线配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// 是否启用运动检测
    pub motion_detection: bool,
    /// 跳帧：每隔 N 帧处理一次（>= 1）
    pub frame_skip: u32,
    /// 告警冷却时间（毫秒）：同一通道两次告警最小间隔
    pub alert_cooldown_ms: u64,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            motion_detection: true,
            frame_skip: 5,
            alert_cooldown_ms: 10_000,
        }
    }
}

/// 流水线处理结果（推给前端的事件载荷）
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PipelineEvent {
    /// 处理进度 / 统计
    Stats {
        source_id: String,
        fps: f32,
        processed: u64,
        detected: u32,
    },
    /// 检测到目标
    Detection {
        source_id: String,
        kind: String,
        confidence: f32,
        bbox: [f32; 4],
        ts: i64,
    },
    /// 告警
    Alert {
        source_id: String,
        level: String, // info / warn / critical
        message: String,
        ts: i64,
    },
}

/// 一个视频源对应的流水线
pub struct Pipeline {
    pub source_id: String,
    pub config: PipelineConfig,
    rx: mpsc::Receiver<FramePacket>,
    sink: mpsc::Sender<PipelineEvent>,
}

impl Pipeline {
    pub fn new(
        source_id: String,
        config: PipelineConfig,
        rx: mpsc::Receiver<FramePacket>,
        sink: mpsc::Sender<PipelineEvent>,
    ) -> Self {
        Self {
            source_id,
            config,
            rx,
            sink,
        }
    }

    /// 在自己的 tokio task 中跑
    pub async fn run(mut self) {
        // TODO: 这里串起来
        //   let mut decoder = Decoder::new(...);
        //   let mut detector = Detector::new(self.config.clone());
        //   let mut analyzer = Analyzer::new(...);
        //   while let Some(packet) = self.rx.recv().await {
        //       let frame = decoder.decode(packet)?;
        //       if self.config.motion_detection {
        //           let det = detector.detect(&frame);
        //           analyzer.feed(det);
        //       }
        //       self.emit_stats(...);
        //   }
        // 占位：等 stream::registry 把帧接上之后再连
        let _ = self.rx.recv().await; // 等到有帧再继续
    }
}

/// 工厂：拿一个 Arc 给 registry 调度
pub type PipelineHandle = Arc<Pipeline>;
