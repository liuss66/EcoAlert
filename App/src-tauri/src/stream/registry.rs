//! 通道注册表
//!
//! 前端调 `add_source` 后，commands 层会调用本模块：
//! - 启动一个 tokio task 拉流
//! - 拉到的帧发给 Pipeline
//! - Pipeline 产生的事件 emit 到前端
//!
//! 当前是占位：把模块结构跑通即可。

use crate::stream::{hls::HlsReader, rtmp::RtmpServer, FramePacket, StreamSpec};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct StreamRegistry {
    inner: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
}

impl StreamRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(HashMap::new()),
        })
    }

    /// 启动一条流：拉帧 → 推给 pipeline
    pub fn start(&self, spec: StreamSpec) -> anyhow::Result<()> {
        let (tx, mut rx) = mpsc::channel::<FramePacket>(64);
        let source_id = spec.source_id.clone();
        let handle = match spec.kind {
            crate::stream::StreamKind::Hls => {
                let r = HlsReader::new(&spec.url);
                let run_source_id = source_id.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = r.run(run_source_id, tx).await;
                })
            }
            crate::stream::StreamKind::Rtmp => {
                let s = RtmpServer::new("0.0.0.0:1935");
                tauri::async_runtime::spawn(async move {
                    let _ = s.run(tx).await;
                })
            }
            crate::stream::StreamKind::WebRtc => {
                // 未来
                return Ok(());
            }
        };
        // 帧消费侧：将来连 pipeline
        tauri::async_runtime::spawn(async move {
            while rx.recv().await.is_some() {
                // TODO: 把帧丢给 pipeline
            }
        });
        self.inner.lock().insert(source_id, handle);
        Ok(())
    }

    pub fn stop(&self, source_id: &str) {
        if let Some(h) = self.inner.lock().remove(source_id) {
            h.abort();
        }
    }
}
