//! 视频解码 / 抽帧。
//!
//! 当前阶段先使用 ffmpeg 子进程按需抽取单帧，避免引入长期运行的解码服务。
//! 后续如果算法频率提高，再替换为常驻 ffmpeg / ffmpeg-next 解码管线。

use crate::pipeline::PipelineConfig;
use std::fs;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

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

pub fn extract_gray_frame_from_url(
    url: &str,
    width: u32,
    height: u32,
    timeout: Duration,
) -> anyhow::Result<DecodedFrame> {
    let output = std::env::temp_dir().join(format!(
        "ecoalert_frame_{}_{}.raw",
        std::process::id(),
        Uuid::new_v4().simple()
    ));

    let mut command = Command::new("ffmpeg");
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-i",
        url,
        "-frames:v",
        "1",
        "-vf",
        &format!("scale={width}:{height},format=gray"),
        "-f",
        "rawvideo",
    ]);
    command.arg(&output);
    command.stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    let mut child = command.spawn()?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&output);
            anyhow::bail!("ffmpeg 抽帧超时");
        }
        thread::sleep(Duration::from_millis(50));
    };

    if !status.success() {
        let _ = fs::remove_file(&output);
        anyhow::bail!("ffmpeg 抽帧失败，退出码: {status}");
    }

    let data = fs::read(&output)?;
    let _ = fs::remove_file(&output);
    let expected = (width * height) as usize;
    if data.len() != expected {
        anyhow::bail!("抽帧大小异常: {} bytes, expected {}", data.len(), expected);
    }

    Ok(DecodedFrame {
        width,
        height,
        pts_ms: chrono::Utc::now().timestamp_millis(),
        data,
    })
}
