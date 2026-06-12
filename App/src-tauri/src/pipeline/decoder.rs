//! 视频解码（占位）
//!
//! 真实实现计划：
//! - 方案 A：`ffmpeg-next` crate，拉 HLS/RTMP/HTTP-flv，输出 YUV/RGB 帧
//! - 方案 B：`opencv` crate 的 `VideoCapture`，优点是能直接走 CUDA 加速
//!
//! 早期阶段不接真实视频时，可以读 `Video/samples/*.mp4` 用 `image` crate 解关键帧。

use crate::pipeline::PipelineConfig;

/// 解码出的单帧（最小可用结构，具体像素格式后续定）
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    pub pts_ms: i64,
    pub data: Vec<u8>, // 灰度图，单通道 8-bit
}

pub struct Decoder {
    _config: PipelineConfig,
}

impl Decoder {
    pub fn new(config: PipelineConfig) -> Self {
        Self { _config: config }
    }

    /// 解码一段压缩视频数据。占位实现：返回空帧。
    /// 真正实现时调用 ffmpeg / opencv。
    pub fn decode(&mut self, _compressed: &[u8]) -> anyhow::Result<Option<DecodedFrame>> {
        Ok(None)
    }
}
