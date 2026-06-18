//! YOLO 目标检测 WebSocket 客户端
//!
//! 与外部 YOLO 检测服务器（FastAPI + WebSocket）保持长连接，循环发送 JPEG
//! 帧并接收 JSON 检测结果。
//! 协议见 G:\project\YOLOv8n\server_ws.py：
//!   - 端点：ws://host:port/ws
//!   - 客户端 → 服务端：二进制 JPEG 字节
//!   - 服务端 → 客户端：JSON 文本 { detections, count, process_ms } 或 { error }
//! 包含图像编码（RGB → JPEG）、连接管理、响应解析和熔断器。

use futures_util::{SinkExt, StreamExt};
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageBuffer, Rgb};
use serde::Deserialize;
use std::io::Cursor;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use super::decoder::DecodedFrame;

// ── 结果类型 ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct YoloDetectResult {
    pub person: bool,
    pub person_confidence: f32,
    pub detections: Vec<YoloDetection>,
    pub process_ms: f32,
}

#[derive(Debug, Clone)]
pub struct YoloDetection {
    pub confidence: f32,
    pub bbox: [f32; 4], // [x, y, w, h] 归一化坐标
}

// ── 熔断器 ────────────────────────────────────────────────────────────────

const CIRCUIT_FAILURE_THRESHOLD: u32 = 2;
const CIRCUIT_COOLDOWN_MS: i64 = 15_000;

#[derive(Debug, Clone)]
pub struct YoloCircuitBreaker {
    consecutive_failures: u32,
    cooldown_until_ms: i64,
}

impl Default for YoloCircuitBreaker {
    fn default() -> Self {
        Self {
            consecutive_failures: 0,
            cooldown_until_ms: 0,
        }
    }
}

impl YoloCircuitBreaker {
    pub fn is_open(&self, now_ms: i64) -> bool {
        self.cooldown_until_ms > now_ms
    }

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.cooldown_until_ms = 0;
    }

    pub fn record_failure(&mut self, now_ms: i64) {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= CIRCUIT_FAILURE_THRESHOLD {
            self.cooldown_until_ms = now_ms + CIRCUIT_COOLDOWN_MS;
            log::warn!(
                "[yolo] 熔断器开启：连续失败 {} 次，冷却 {} 秒",
                self.consecutive_failures,
                CIRCUIT_COOLDOWN_MS / 1000
            );
        }
    }
}

// ── 服务端响应 ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DetectResponse {
    #[serde(default)]
    detections: Vec<RawDetection>,
    #[allow(dead_code)]
    #[serde(default)]
    count: i32,
    #[allow(dead_code)]
    #[serde(default)]
    process_ms: f32,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct RawDetection {
    #[allow(dead_code)]
    class_id: i32,
    #[allow(dead_code)]
    class_name: String,
    confidence: f32,
    bbox: [f32; 4],
}

// ── 长连接客户端 ──────────────────────────────────────────────────────────

type YoloStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// 每源一个连接。空连接时按需建连，发送出错时清空等待重连。
pub struct YoloClient {
    api_base: String,
    stream: Mutex<Option<YoloStream>>,
    /// 上次使用的 URL（用于检测地址是否变化，决定是否重连）
    connected_url: Mutex<Option<String>>,
}

impl YoloClient {
    pub fn new(api_base: String) -> Self {
        Self {
            api_base,
            stream: Mutex::new(None),
            connected_url: Mutex::new(None),
        }
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    /// 推出一帧并等待结果。整体由调用方加 tokio::time::timeout 保护。
    pub async fn detect_frame(
        &self,
        jpeg_bytes: Vec<u8>,
    ) -> Result<YoloDetectResult, String> {
        let url = self.ws_url()?;

        // 懒建连：第一次调用或上次连接断开了
        {
            let mut guard = self.stream.lock().await;
            let need_connect = guard.is_none();
            if need_connect {
                // 建连加 3s 超时，避免 DNS/网络问题时无限等待
                let (ws, _resp) = tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    connect_async(&url),
                )
                .await
                .map_err(|_| "连接超时（>3s）".to_string())?
                .map_err(|e| format!("连接失败: {e}"))?;
                *guard = Some(ws);
                *self.connected_url.lock().await = Some(url.clone());
            }
        }

        // 发送二进制帧
        let send_result = {
            let mut guard = self.stream.lock().await;
            let ws = guard.as_mut().ok_or_else(|| "连接已断开".to_string())?;
            ws.send(Message::Binary(jpeg_bytes.into()))
                .await
                .map_err(|e| format!("发送失败: {e}"))
        };
        if let Err(e) = send_result {
            self.drop_stream().await;
            return Err(e);
        }

        // 等待响应：循环跳过 Ping/Pong/Frame 等控制消息，直到拿到 Text/Close 或出错。
        // tungstenite 0.29 的 Message 多了 Ping/Pong/Frame 变体，需要主动忽略而不是报错。
        let recv_result = {
            let mut guard = self.stream.lock().await;
            let ws = guard.as_mut().ok_or_else(|| "连接已断开".to_string())?;
            loop {
                match ws.next().await {
                    Some(Ok(Message::Text(text))) => break Ok(text),
                    Some(Ok(Message::Close(_))) => {
                        break Err("服务器主动关闭".to_string());
                    }
                    Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {
                        // 控制帧，自动响应由 tungstenite 完成（已发送 Pong），继续等数据
                        log::debug!("[yolo] 收到控制帧，继续等待响应");
                        continue;
                    }
                    Some(Ok(other)) => {
                        break Err(format!("收到非预期消息类型: {other:?}"));
                    }
                    Some(Err(e)) => break Err(format!("接收失败: {e}")),
                    None => break Err("服务器关闭连接".to_string()),
                }
            }
        };
        let text = match recv_result {
            Ok(t) => t,
            Err(e) => {
                self.drop_stream().await;
                return Err(e);
            }
        };

        // 解析响应
        let resp: DetectResponse = serde_json::from_str(&text)
            .map_err(|e| format!("解析响应失败: {e} (text={})", truncate(&text, 200)))?;
        if let Some(err) = resp.error {
            return Err(format!("服务端错误: {err}"));
        }

        let detections: Vec<YoloDetection> = resp
            .detections
            .into_iter()
            .map(|d| YoloDetection {
                confidence: d.confidence,
                bbox: d.bbox,
            })
            .collect();
        let person = !detections.is_empty();
        let person_confidence = detections
            .iter()
            .map(|d| d.confidence)
            .fold(0.0_f32, f32::max);
        Ok(YoloDetectResult {
            person,
            person_confidence,
            detections,
            process_ms: resp.process_ms,
        })
    }

    /// 主动断开连接（健康检查用、配置变更时重置）
    pub async fn disconnect(&self) {
        self.drop_stream().await;
    }

    async fn drop_stream(&self) {
        let mut guard = self.stream.lock().await;
        if let Some(mut ws) = guard.take() {
            // 给关闭操作加 2s 超时，避免服务器无响应时卡死整个流水线
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), ws.close(None)).await;
        }
        *self.connected_url.lock().await = None;
    }

    fn ws_url(&self) -> Result<String, String> {
        let trimmed = self.api_base.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            return Err("YOLO 服务器地址为空".into());
        }
        // 允许 http:// 自动转 ws://（用户经常复制 HTTP 风格地址）
        let normalized = if let Some(rest) = trimmed.strip_prefix("http://") {
            format!("ws://{rest}")
        } else if let Some(rest) = trimmed.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
            trimmed.to_string()
        } else {
            // 裸 host:port
            format!("ws://{trimmed}")
        };
        // 强制 /ws 路径（覆盖用户填的路径，避免误连）
        let base = normalized.trim_end_matches('/');
        let without_ws = base.trim_end_matches("/ws");
        Ok(format!("{without_ws}/ws"))
    }
}

// ── 顶层便捷函数（保持向后兼容的 detect_person 接口） ───────────────────

/// 把 RGB 帧编码为 JPEG 字节，便于外部直接传 raw 字节给 YOLO 客户端。
pub fn encode_frame_as_jpeg(frame: &DecodedFrame) -> Result<Vec<u8>, String> {
    if frame.rgb.is_empty() {
        return Err("帧无 RGB 数据，无法编码".into());
    }
    let img =
        ImageBuffer::<Rgb<u8>, Vec<u8>>::from_raw(frame.width, frame.height, frame.rgb.clone())
            .ok_or_else(|| "RGB 帧大小异常".to_string())?;
    let dyn_img = DynamicImage::ImageRgb8(img);

    let mut buf = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut buf, 85);
    encoder
        .encode_image(&dyn_img)
        .map_err(|e| format!("JPEG 编码失败: {e}"))?;
    Ok(buf.into_inner())
}

// ── 工具函数 ──────────────────────────────────────────────────────────────

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

// 编译期 sanity check：JPEG 编码必须能跑通（实际图片数据由测试覆盖）
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::decoder::DecodedFrame;

    fn dummy_frame(value: u8) -> DecodedFrame {
        DecodedFrame {
            width: 8,
            height: 8,
            pts_ms: 0,
            data: vec![value; 8 * 8],
            rgb: vec![value; 8 * 8 * 3],
        }
    }

    #[test]
    fn encode_jpeg_works_on_dummy_frame() {
        let jpeg = encode_frame_as_jpeg(&dummy_frame(128)).unwrap();
        assert!(!jpeg.is_empty());
        // JPEG magic: FF D8
        assert_eq!(jpeg[0], 0xFF);
        assert_eq!(jpeg[1], 0xD8);
    }

    #[test]
    fn ws_url_normalizes_http_to_ws() {
        let c = YoloClient::new("http://localhost:8090".into());
        assert_eq!(c.ws_url().unwrap(), "ws://localhost:8090/ws");
    }

    #[test]
    fn ws_url_appends_ws_path() {
        let c = YoloClient::new("ws://localhost:8090".into());
        assert_eq!(c.ws_url().unwrap(), "ws://localhost:8090/ws");
    }

    #[test]
    fn ws_url_handles_trailing_slash_and_ws() {
        let c = YoloClient::new("ws://localhost:8090/ws/".into());
        assert_eq!(c.ws_url().unwrap(), "ws://localhost:8090/ws");
    }

    #[test]
    fn ws_url_rejects_empty() {
        let c = YoloClient::new("".into());
        assert!(c.ws_url().is_err());
    }

    #[test]
    fn ws_url_supports_https() {
        let c = YoloClient::new("https://example.com:8443".into());
        assert_eq!(c.ws_url().unwrap(), "wss://example.com:8443/ws");
    }
}
