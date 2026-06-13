//! 推流承接模块
//!
//! 职责：把外部推过来的流（HLS、RTMP、未来的 WebRTC）拉下来，
//! 拆成帧后丢给 `pipeline` 处理。
//!
//! 跟前端的关系：前端只配置 `url`（HLS / RTMP / 本地文件），
//! 具体的拉流协议由本模块决定。

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

pub mod hls;
pub mod registry;
pub mod rtmp;

/// 一帧压缩数据（解码前）
#[derive(Debug, Clone)]
pub struct FramePacket {
    pub source_id: String,
    pub pts_ms: i64,
    pub data: Vec<u8>,
    pub keyframe: bool,
}

/// 流的类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StreamKind {
    Hls,
    Rtmp,
    /// 预留：未来 Tools 用 WebRTC 推
    WebRtc,
}

/// 流的描述（来自前端视频源配置）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSpec {
    pub source_id: String,
    pub kind: StreamKind,
    pub url: String,
}
