//! HLS 拉流（占位）
//!
//! 真实实现：拉 m3u8 → 顺序下载 ts 分片 → 喂给 ffmpeg 解码 → 推 FramePacket
//!
//! 当前仅占位，后续接 ffmpeg / 第三方 crate。

use crate::stream::FramePacket;
use tokio::sync::mpsc;

pub struct HlsReader {
    url: String,
}

impl HlsReader {
    pub fn new(url: impl Into<String>) -> Self { Self { url: url.into() } }

    /// 在自己的 task 里跑：拉 m3u8 → 持续输出 FramePacket 到 `tx`
    pub async fn run(self, _source_id: String, _tx: mpsc::Sender<FramePacket>) -> anyhow::Result<()> {
        // TODO: reqwest 拉 m3u8，循环下载 ts
        // 占位：永远阻塞，等 stream::registry 取消
        futures::future::pending::<()>().await;
        Ok(())
    }
}
