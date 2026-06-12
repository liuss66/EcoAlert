//! RTMP 接收（占位）
//!
//! Tools 推流器用 ffmpeg 把 mp4 推 RTMP 到本地端口，App 在这里收。
//! 真实实现：rtsp-rs / rtmp crate 监听端口。

use crate::stream::FramePacket;
use std::future;
use tokio::sync::mpsc;

pub struct RtmpServer {
    bind: String, // "0.0.0.0:1935"
}

impl RtmpServer {
    pub fn new(bind: impl Into<String>) -> Self {
        Self { bind: bind.into() }
    }

    pub async fn run(self, _tx: mpsc::Sender<FramePacket>) -> anyhow::Result<()> {
        // TODO: 监听 RTMP 端口
        future::pending::<()>().await;
        Ok(())
    }
}
