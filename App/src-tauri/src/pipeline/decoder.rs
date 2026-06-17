//! 视频解码 / 抽帧。
//!
//! 当前阶段先使用 ffmpeg 子进程按需抽取单帧，避免引入长期运行的解码服务。
//! 后续如果算法频率提高，再替换为常驻 ffmpeg / ffmpeg-next 解码管线。

use crate::pipeline::PipelineConfig;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
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
    pub rgb: Vec<u8>,  // RGB 图，三通道 8-bit，用于判断彩色 / 红外黑白模式
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

pub fn resolve_media_tool(name: &str) -> PathBuf {
    let exe_name = if cfg!(windows) && !name.ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            let candidate = dir.join(&exe_name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from(OsString::from(exe_name))
}

fn media_tool_spawn_error(tool: &str, err: std::io::Error) -> anyhow::Error {
    if err.kind() == std::io::ErrorKind::NotFound {
        anyhow::anyhow!(
            "未找到 {tool}，请将 {tool}.exe 和 ffprobe.exe 放到程序目录，或把 ffmpeg 安装目录加入 PATH"
        )
    } else {
        anyhow::anyhow!("{tool} 启动失败: {err}")
    }
}

pub fn extract_gray_frame_from_url(
    url: &str,
    width: u32,
    height: u32,
    timeout: Duration,
) -> anyhow::Result<DecodedFrame> {
    extract_gray_frame_from_url_at(url, width, height, timeout, None)
}

pub fn extract_gray_frame_from_url_at(
    url: &str,
    width: u32,
    height: u32,
    timeout: Duration,
    seek_secs: Option<f64>,
) -> anyhow::Result<DecodedFrame> {
    let output = std::env::temp_dir().join(format!(
        "ecoalert_frame_{}_{}.raw",
        std::process::id(),
        Uuid::new_v4().simple()
    ));

    let is_rtsp = url.starts_with("rtsp://") || url.starts_with("rtsps://");

    let mut command = Command::new(resolve_media_tool("ffmpeg"));
    command.args(["-hide_banner", "-loglevel", "error", "-y"]);
    // 限制流探测时间，默认 5 秒太长，是每次抽帧的固定开销。
    // 500ms 足以识别绝大多数摄像头 / MP4 的编码格式。
    command.args([
        "-analyzeduration",
        "500000",
        "-probesize",
        "5000000",
    ]);

    // RTSP 流强制使用 TCP 传输，避免 UDP 丢包导致花屏或抽帧失败
    if is_rtsp {
        command.args(["-rtsp_transport", "tcp"]);
    }

    let seek_arg = seek_secs.map(|value| format!("{:.3}", value.max(0.0)));
    command.args(["-i", url]);
    // 对 MP4 等文件类媒体，-ss 放在 -i 后面做精确Seek（慢但准确到帧级别）。
    // 旧版 -ss 在 -i 前面只定位到最近关键帧，连续帧可能完全相同导致帧差为零。
    if let Some(seek) = &seek_arg {
        command.args(["-ss", seek]);
    }
    command.args([
        "-frames:v",
        "1",
        "-vf",
        &format!("scale={width}:{height},format=rgb24"),
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

    let mut child = command
        .spawn()
        .map_err(|err| media_tool_spawn_error("ffmpeg", err))?;
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

    let rgb = fs::read(&output)?;
    let _ = fs::remove_file(&output);
    let expected = (width * height * 3) as usize;
    if rgb.len() != expected {
        anyhow::bail!("抽帧大小异常: {} bytes, expected {}", rgb.len(), expected);
    }
    let data = rgb_to_gray(&rgb);

    Ok(DecodedFrame {
        width,
        height,
        pts_ms: chrono::Utc::now().timestamp_millis(),
        data,
        rgb,
    })
}

pub fn probe_media_duration_secs(url: &str, timeout: Duration) -> anyhow::Result<Option<f64>> {
    let mut command = Command::new(resolve_media_tool("ffprobe"));
    command.args([
        "-v",
        "error",
        "-show_entries",
        "format=duration",
        "-of",
        "default=noprint_wrappers=1:nokey=1",
        url,
    ]);
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    let mut child = command
        .spawn()
        .map_err(|err| media_tool_spawn_error("ffprobe", err))?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("ffprobe 探测时长超时");
        }
        thread::sleep(Duration::from_millis(50));
    };
    if !status.success() {
        anyhow::bail!("ffprobe 探测时长失败，退出码: {status}");
    }
    let mut text = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        stdout.read_to_string(&mut text)?;
    }
    let duration = text.trim().parse::<f64>().ok().filter(|value| *value > 0.0);
    Ok(duration)
}

fn rgb_to_gray(rgb: &[u8]) -> Vec<u8> {
    rgb.chunks_exact(3)
        .map(|px| {
            let r = px[0] as u32;
            let g = px[1] as u32;
            let b = px[2] as u32;
            ((77 * r + 150 * g + 29 * b) >> 8) as u8
        })
        .collect()
}
