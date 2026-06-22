use crate::pipeline::decoder::DecodedFrame;
use crate::store::AlgorithmConfig;
use anyhow::{anyhow, Context};
use image::codecs::jpeg::JpegEncoder;
use image::{imageops::FilterType, DynamicImage, ImageBuffer, Rgb};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct VlmDetection {
    pub has_person: bool,
    pub confidence: f32,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VlmUsage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
    #[serde(default)]
    pub prompt_cached_tokens: u32,
    #[serde(default)]
    pub completion_cached_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VlmTestResult {
    pub reply: String,
    pub usage: Option<VlmUsage>,
    pub request_url: String,
    pub request_body: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<RawUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunk {
    #[serde(default)]
    choices: Vec<ChatChunkChoice>,
    #[serde(default)]
    usage: Option<RawUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChunkChoice {
    #[serde(default)]
    delta: ChatChunkDelta,
    /// 兼容部分模型在最后一个 chunk 用 message 代替 delta
    #[serde(default)]
    message: Option<ChatMessage>,
}

#[derive(Debug, Deserialize, Default)]
struct ChatChunkDelta {
    #[serde(default)]
    content: Option<serde_json::Value>,
    /// Qwen3 等模型在思考阶段使用 reasoning_content，正式回答用 content
    #[serde(default)]
    reasoning_content: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct RawUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
    #[serde(default)]
    prompt_tokens_details: Option<TokenDetails>,
    #[serde(default)]
    completion_tokens_details: Option<TokenDetails>,
}

#[derive(Debug, Deserialize)]
struct TokenDetails {
    #[serde(default)]
    cached_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct ModelDetectionResult {
    #[serde(default)]
    has_person: bool,
    #[serde(default)]
    detections: Vec<ModelDetection>,
}

#[derive(Debug, Deserialize)]
struct ModelDetection {
    #[serde(default)]
    confidence: f32,
}

pub async fn analyze_person(
    config: &AlgorithmConfig,
    frame: &DecodedFrame,
) -> anyhow::Result<VlmDetection> {
    let api_base = config.vlm_api_base.trim().trim_end_matches('/');
    let api_key = config.vlm_api_key.trim();
    let model = config.vlm_model.trim();
    if api_base.is_empty() || api_key.is_empty() || model.is_empty() {
        anyhow::bail!("VLM API 地址、API Key、模型名称不能为空");
    }

    let image_url = frame_to_jpeg_data_url(frame)?;
    let body = build_vision_request_body(config, model, image_url);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("node")
        .build()?;
    let url = chat_completions_url(api_base);
    let response = client
        .post(&url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "*/*")
        .header(AUTHORIZATION, format!("Bearer {api_key}"))
        .json(&body)
        .send()
        .await
        .with_context(|| format!("VLM 请求发送失败 (model={model}, api={api_base})"))?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    // 用 bytes() 而不是 text()：部分 API 在 chunked transfer / event-stream 下
    // text() 可能返回空串，bytes() 更可靠。
    let raw_bytes = response.bytes().await.unwrap_or_default();
    let text = String::from_utf8_lossy(&raw_bytes).into_owned();
    let is_stream_response = content_type.contains("event-stream")
        || content_type.contains("text/event-stream")
        || looks_like_sse_response(&text);
    if is_stream_response || should_retry_as_stream(status, &text, &body) {
        // 如果已经是流式响应（Content-Type: event-stream），直接解析，不用再重试
        let stream_text = if is_stream_response && !text.is_empty() {
            text.clone()
        } else {
            let stream_body = with_stream_enabled(&body);
            let response = client
                .post(&url)
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "text/event-stream")
                .header(AUTHORIZATION, format!("Bearer {api_key}"))
                .json(&stream_body)
                .send()
                .await
                .with_context(|| format!("VLM 流式重试发送失败 (model={model}, api={api_base})"))?;
            let status = response.status();
            let stream_bytes = response.bytes().await.unwrap_or_default();
            let stream_text = String::from_utf8_lossy(&stream_bytes).into_owned();
            if !status.is_success() {
                anyhow::bail!(
                    "{}",
                    format_vlm_http_error(status, &stream_text, &url, &stream_body)
                );
            }
            if stream_text.is_empty() {
                anyhow::bail!(
                    "VLM 流式响应体为空 (model={model}, status={status}, url={url})"
                );
            }
            stream_text
        };
        let content = match parse_streaming_chat_content(&stream_text) {
            Ok(c) => c,
            Err(e) => {
                log::debug!(
                    "VLM streaming parse failed for model={model} api={api_base}, raw response ({} bytes): {}",
                    stream_text.len(),
                    if stream_text.len() > 500 { format!("{}...", &stream_text[..500]) } else { stream_text.clone() }
                );
                anyhow::bail!("VLM 流式响应解析失败 (model={model}): {e}");
            }
        };
        return parse_detection_content(&content);
    }
    if !status.is_success() {
        anyhow::bail!("{}", format_vlm_http_error(status, &text, &url, &body));
    }

    let parsed: ChatCompletionResponse =
        serde_json::from_str(&text).with_context(|| {
            let snippet = if text.len() > 200 { format!("{}...", &text[..200]) } else { text.clone() };
            format!("VLM 响应不是 OpenAI-compatible JSON (model={model}), 原始响应: {snippet}")
        })?;
    let content = parsed
        .choices
        .first()
        .map(|choice| message_content_to_string(&choice.message.content))
        .ok_or_else(|| anyhow!("VLM 响应缺少 choices[0].message.content"))?;
    parse_detection_content(&content)
}

pub async fn test_connection(config: &AlgorithmConfig) -> anyhow::Result<VlmTestResult> {
    let body = serde_json::json!({
        "model": config.vlm_model.trim(),
        "messages": [{ "role": "user", "content": "Hi" }],
        "max_tokens": 16,
    });
    let (parsed, request_url, request_body) = call_chat_completion(config, body).await?;
    let content = parsed
        .choices
        .first()
        .map(|choice| message_content_to_string(&choice.message.content))
        .unwrap_or_default();
    Ok(VlmTestResult {
        reply: content,
        usage: parsed.usage.map(Into::into),
        request_url,
        request_body,
    })
}

/// 用当前配置对一帧画面做真实人员检测测试，用于在前端验证 VLM 图片识别是否正常。
pub async fn test_vision(
    config: &AlgorithmConfig,
    frame: &DecodedFrame,
) -> anyhow::Result<VlmTestResult> {
    let model = config.vlm_model.trim();
    let image_url = frame_to_jpeg_data_url(frame)?;
    let body = build_vision_request_body(config, model, image_url);
    let (parsed, request_url, request_body) = call_chat_completion(config, body).await?;
    let content = parsed
        .choices
        .first()
        .map(|choice| message_content_to_string(&choice.message.content))
        .unwrap_or_default();
    Ok(VlmTestResult {
        reply: content,
        usage: parsed.usage.map(Into::into),
        request_url,
        request_body,
    })
}

async fn call_chat_completion(
    config: &AlgorithmConfig,
    body: serde_json::Value,
) -> anyhow::Result<(ChatCompletionResponse, String, serde_json::Value)> {
    let api_base = config.vlm_api_base.trim().trim_end_matches('/');
    let api_key = config.vlm_api_key.trim();
    let model = config.vlm_model.trim();
    if api_base.is_empty() || api_key.is_empty() || model.is_empty() {
        anyhow::bail!("VLM API 地址、API Key、模型名称不能为空");
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("node")
        .build()?;
    let url = chat_completions_url(api_base);
    let request_url = url.clone();
    let request_body = body.clone();
    let response = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "*/*")
        .header(AUTHORIZATION, format!("Bearer {api_key}"))
        .json(&body)
        .send()
        .await
        .with_context(|| format!("VLM 请求发送失败 (model={model}, api={api_base})"))?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let raw_bytes = response.bytes().await.unwrap_or_default();
    let text = String::from_utf8_lossy(&raw_bytes).into_owned();
    let is_stream_response = content_type.contains("event-stream")
        || content_type.contains("text/event-stream")
        || looks_like_sse_response(&text);
    if is_stream_response || should_retry_as_stream(status, &text, &request_body) {
        let stream_text = if is_stream_response && !text.is_empty() {
            text.clone()
        } else {
            let stream_body = with_stream_enabled(&request_body);
            let response = client
                .post(&request_url)
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "text/event-stream")
                .header(AUTHORIZATION, format!("Bearer {api_key}"))
                .json(&stream_body)
                .send()
                .await
                .with_context(|| format!("VLM 流式重试发送失败 (model={model}, api={api_base})"))?;
            let status = response.status();
            let stream_bytes = response.bytes().await.unwrap_or_default();
            let stream_text = String::from_utf8_lossy(&stream_bytes).into_owned();
            if !status.is_success() {
                anyhow::bail!(
                    "{}",
                    format_vlm_http_error(status, &stream_text, &request_url, &stream_body)
                );
            }
            if stream_text.is_empty() {
                anyhow::bail!(
                    "VLM 流式响应体为空 (model={model}, status={status}, url={request_url})"
                );
            }
            stream_text
        };
        let content = match parse_streaming_chat_content(&stream_text) {
            Ok(c) => c,
            Err(e) => {
                log::debug!(
                    "VLM streaming parse failed for model={model} api={api_base}, raw response ({} bytes): {}",
                    stream_text.len(),
                    if stream_text.len() > 500 { format!("{}...", &stream_text[..500]) } else { stream_text.clone() }
                );
                anyhow::bail!("VLM 流式响应解析失败 (model={model}): {e}");
            }
        };
        return Ok((
            ChatCompletionResponse {
                choices: vec![ChatChoice {
                    message: ChatMessage {
                        content: serde_json::Value::String(content),
                    },
                }],
                usage: parse_streaming_chat_usage(&stream_text),
            },
            request_url,
            request_body,
        ));
    }
    if !status.is_success() {
        anyhow::bail!(
            "{}",
            format_vlm_http_error(status, &text, &request_url, &request_body)
        );
    }
    let parsed = serde_json::from_str(&text).with_context(|| {
        let snippet = if text.len() > 200 { format!("{}...", &text[..200]) } else { text.clone() };
        format!("VLM 响应不是 OpenAI-compatible JSON (model={model}), 原始响应: {snippet}")
    })?;
    Ok((parsed, request_url, request_body))
}

/// 判断响应体是否是 SSE（Server-Sent Events）流式格式。
///
/// 必须逐行检查 `data:` 是否出现在行首，而不是用 `text.contains("data:")` 做全文模糊匹配。
/// 后者会在普通 JSON 响应包含 `data:` 子串时（如 data URL、base64 数据、模型回复文本）
/// 产生误判，导致代码错误地进入流式重试分支，进而触发"流式响应体为空"。
fn looks_like_sse_response(text: &str) -> bool {
    text.lines().any(|line| line.trim_start().starts_with("data:"))
}

fn should_retry_as_stream(
    status: reqwest::StatusCode,
    text: &str,
    request_body: &serde_json::Value,
) -> bool {
    // 已经开启 stream 就不再重试
    if request_body
        .get("stream")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return false;
    }
    // 服务端明确要求 stream: true
    if status.as_u16() == 400
        && (text.contains("stream_options") || text.contains("stream: true") || text.contains("\"stream\""))
    {
        return true;
    }
    // 部分 API 返回 400 但没有明确关键词，也尝试流式
    if status.as_u16() == 400 && text.contains("stream") {
        return true;
    }
    false
}

fn with_stream_enabled(request_body: &serde_json::Value) -> serde_json::Value {
    let mut body = request_body.clone();
    if let Some(map) = body.as_object_mut() {
        map.insert("stream".into(), serde_json::Value::Bool(true));
        // 部分 API（如 OpenAI 兼容接口）要求 stream_options 来获取 usage
        map.insert(
            "stream_options".into(),
            serde_json::json!({ "include_usage": true }),
        );
    }
    body
}

fn parse_streaming_chat_content(text: &str) -> anyhow::Result<String> {
    let chunks = parse_streaming_chat_chunks(text)?;
    let mut content = String::new();
    let mut reasoning = String::new();
    for chunk in &chunks {
        for choice in &chunk.choices {
            if let Some(delta_content) = &choice.delta.content {
                content.push_str(&stream_delta_content_to_string(delta_content));
            }
            if let Some(delta_reasoning) = &choice.delta.reasoning_content {
                reasoning.push_str(&stream_delta_content_to_string(delta_reasoning));
            }
            // 兼容部分模型在最后一个 chunk 用 message 而不是 delta
            if content.is_empty() {
                if let Some(msg) = choice.message.as_ref() {
                    let msg_content = message_content_to_string(&msg.content);
                    if !msg_content.is_empty() {
                        content.push_str(&msg_content);
                    }
                }
            }
        }
    }
    let content = content.trim().to_string();
    if content.is_empty() {
        // Qwen3 等思考模型可能只输出了 reasoning_content 而 content 为空
        if !reasoning.trim().is_empty() {
            return Ok(reasoning.trim().to_string());
        }
        // Fallback: try parsing as regular (non-streaming) ChatCompletion response
        if let Ok(parsed) = serde_json::from_str::<ChatCompletionResponse>(text.trim()) {
            if let Some(choice) = parsed.choices.first() {
                let fallback_content = message_content_to_string(&choice.message.content);
                if !fallback_content.is_empty() {
                    return Ok(fallback_content);
                }
            }
        }
        let snippet = if text.len() > 500 {
            format!("{}...", &text[..500])
        } else if text.is_empty() {
            "(空响应)".to_string()
        } else {
            text.to_string()
        };
        anyhow::bail!("流式响应缺少 delta.content，原始响应: {snippet}");
    }
    Ok(content)
}

fn parse_streaming_chat_usage(text: &str) -> Option<RawUsage> {
    parse_streaming_chat_chunks(text)
        .ok()?
        .into_iter()
        .filter_map(|chunk| chunk.usage)
        .last()
}

fn parse_streaming_chat_chunks(text: &str) -> anyhow::Result<Vec<ChatCompletionChunk>> {
    let mut chunks = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        chunks.push(serde_json::from_str(data)?);
    }
    Ok(chunks)
}

fn chat_completions_url(api_base: &str) -> String {
    if api_base.ends_with("/chat/completions") {
        api_base.to_string()
    } else {
        format!("{api_base}/chat/completions")
    }
}

fn build_vision_request_body(
    config: &AlgorithmConfig,
    model: &str,
    image_url: String,
) -> serde_json::Value {
    let prompt = config.vlm_prompt.trim();
    let max_tokens = config.vlm_max_tokens.max(16);
    let temperature = config.vlm_temperature.clamp(0.0, 2.0);
    serde_json::json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "image_url", "image_url": { "url": image_url } },
                { "type": "text", "text": prompt },
            ],
        }],
        "max_tokens": max_tokens,
        "temperature": temperature,
    })
}

fn format_vlm_http_error(
    status: reqwest::StatusCode,
    text: &str,
    request_url: &str,
    request_body: &serde_json::Value,
) -> String {
    let detail = text.trim();
    let request = format_request_debug(request_url, request_body);
    if status.as_u16() == 405 && detail.contains("Coding Plan is currently only available") {
        return format!(
            "VLM 请求失败 HTTP {status}: 服务端返回 Coding Plan 限制。当前请求已按 Algo/vlm-detector 的文本测试格式发送。请把下面的请求地址和请求体与 standalone 工具配置逐字对照。\n{request}\n原始响应: {detail}"
        );
    }
    if status.as_u16() == 404 && detail.contains("Not Found") {
        return format!(
            "VLM 请求失败 HTTP {status}: API Base 可能不是 OpenAI-compatible base URL。请填写到 /v1 为止，或直接填写完整 /chat/completions URL。\n{request}\n原始响应: {detail}"
        );
    }
    format!("VLM 请求失败 HTTP {status}: {detail}\n{request}")
}

fn format_request_debug(request_url: &str, request_body: &serde_json::Value) -> String {
    let body =
        serde_json::to_string_pretty(request_body).unwrap_or_else(|_| request_body.to_string());
    format!("请求地址: {request_url}\n请求体: {body}")
}

impl From<RawUsage> for VlmUsage {
    fn from(value: RawUsage) -> Self {
        Self {
            prompt_tokens: value.prompt_tokens,
            completion_tokens: value.completion_tokens,
            total_tokens: value.total_tokens,
            prompt_cached_tokens: value
                .prompt_tokens_details
                .map(|details| details.cached_tokens)
                .unwrap_or_default(),
            completion_cached_tokens: value
                .completion_tokens_details
                .map(|details| details.cached_tokens)
                .unwrap_or_default(),
        }
    }
}

fn frame_to_jpeg_data_url(frame: &DecodedFrame) -> anyhow::Result<String> {
    let img =
        ImageBuffer::<Rgb<u8>, Vec<u8>>::from_raw(frame.width, frame.height, frame.rgb.clone())
            .ok_or_else(|| anyhow!("RGB 帧大小异常"))?;
    let mut dyn_img = DynamicImage::ImageRgb8(img);
    let max_side = frame.width.max(frame.height);
    if max_side > 1280 {
        let scale = 1280.0 / max_side as f32;
        let width = (frame.width as f32 * scale).round().max(1.0) as u32;
        let height = (frame.height as f32 * scale).round().max(1.0) as u32;
        dyn_img = dyn_img.resize(width, height, FilterType::Triangle);
    }

    let mut encoded = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut encoded, 85);
    encoder.encode_image(&dyn_img)?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64_encode(encoded.get_ref())
    ))
}

fn message_content_to_string(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(text) => text.trim().to_string(),
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(|value| value.as_str())
                    .or_else(|| item.get("content").and_then(|value| value.as_str()))
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string(),
        other => other.to_string(),
    }
}

fn stream_delta_content_to_string(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(text) => text.to_string(),
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(|value| value.as_str())
                    .or_else(|| item.get("content").and_then(|value| value.as_str()))
            })
            .collect::<Vec<_>>()
            .join(""),
        other => other.to_string(),
    }
}

fn parse_detection_content(content: &str) -> anyhow::Result<VlmDetection> {
    let result = parse_detection_json(content)
        .or_else(|| extract_code_block(content).and_then(|text| parse_detection_json(&text)))
        .or_else(|| extract_braced_json(content).and_then(|text| parse_detection_json(&text)))
        .unwrap_or_else(|| ModelDetectionResult {
            has_person: content.to_ascii_lowercase().contains("yes")
                || content.contains('有')
                || content.contains('人'),
            detections: vec![],
        });
    let confidence = result
        .detections
        .iter()
        .map(|det| det.confidence)
        .fold(if result.has_person { 0.7 } else { 0.0 }, f32::max)
        .clamp(0.0, 1.0);
    Ok(VlmDetection {
        has_person: result.has_person,
        confidence,
        raw: content.to_string(),
    })
}

fn parse_detection_json(text: &str) -> Option<ModelDetectionResult> {
    serde_json::from_str(text.trim()).ok()
}

fn extract_code_block(text: &str) -> Option<String> {
    let start = text.find("```")?;
    let rest = &text[start + 3..];
    let rest = rest.strip_prefix("json").unwrap_or(rest).trim_start();
    let end = rest.find("```")?;
    Some(rest[..end].trim().to_string())
}

fn extract_braced_json(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| text[start..=end].to_string())
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_code_block() {
        let parsed = parse_detection_content(
            "```json\n{\"has_person\":true,\"detections\":[{\"confidence\":0.88}]}\n```",
        )
        .unwrap();
        assert!(parsed.has_person);
        assert_eq!(parsed.confidence, 0.88);
    }

    #[test]
    fn parses_plain_negative_json() {
        let parsed = parse_detection_content("{\"has_person\":false,\"detections\":[]}").unwrap();
        assert!(!parsed.has_person);
        assert_eq!(parsed.confidence, 0.0);
    }

    #[test]
    fn vision_body_matches_standalone_detector_request() {
        let cfg = AlgorithmConfig {
            vlm_api_base: "https://ai.hirain.com/lm/code/v1".into(),
            vlm_model: "qwen3.6-plus".into(),
            vlm_prompt: "detect".into(),
            vlm_max_tokens: 64,
            vlm_temperature: 0.1,
            ..AlgorithmConfig::default()
        };
        let body =
            build_vision_request_body(&cfg, &cfg.vlm_model, "data:image/jpeg;base64,abc".into());
        assert_eq!(body["model"], "qwen3.6-plus");
        assert_eq!(body["max_tokens"], 64);
        let temperature = body["temperature"].as_f64().unwrap();
        assert!((temperature - 0.1).abs() < 0.000_001);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["type"], "image_url");
        assert_eq!(
            body["messages"][0]["content"][0]["image_url"]["url"],
            "data:image/jpeg;base64,abc"
        );
        assert_eq!(body["messages"][0]["content"][1]["type"], "text");
        assert_eq!(body["messages"][0]["content"][1]["text"], "detect");
        assert!(body.get("stream").is_none());
        assert!(body.get("stream_options").is_none());
        assert!(body.get("thinking").is_none());
        assert!(body.get("max_completion_tokens").is_none());
    }

    #[test]
    fn chat_url_matches_standalone_detector_joining() {
        assert_eq!(
            chat_completions_url("https://ai.hirain.com/lm/code/v1"),
            "https://ai.hirain.com/lm/code/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://api.openai.com/v1/chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://dashscope.aliyuncs.com/v1"),
            "https://dashscope.aliyuncs.com/v1/chat/completions"
        );
    }

    #[test]
    fn parses_streaming_chat_content() {
        let text = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}],\"usage\":{\"total_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        );
        assert_eq!(parse_streaming_chat_content(text).unwrap(), "Hello world");
        assert_eq!(parse_streaming_chat_usage(text).unwrap().total_tokens, 3);
    }

    #[test]
    fn looks_like_sse_detects_real_sse() {
        assert!(looks_like_sse_response(
            "data: {\"choices\":[]}\n\ndata: [DONE]\n\n"
        ));
        assert!(looks_like_sse_response(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}"
        ));
        // 行首有空白也算 SSE
        assert!(looks_like_sse_response(
            "  data: {\"choices\":[]}\n\n"
        ));
    }

    #[test]
    fn looks_like_sse_rejects_json_with_data_substring() {
        // 普通 JSON 响应，内容中包含 "data:" 子串 —— 不应被误判为 SSE
        assert!(!looks_like_sse_response(
            r#"{"choices":[{"message":{"content":"see data: image"}}]}"#
        ));
        // JSON 中包含 data URL 也不应被误判
        assert!(!looks_like_sse_response(
            r#"{"choices":[{"message":{"content":"url is data:image/jpeg;base64,abc"}}]}"#
        ));
        // 纯 JSON 响应，没有任何 SSE 行
        assert!(!looks_like_sse_response(
            r#"{"id":"chatcmpl-1","choices":[{"message":{"content":"ok"}}]}"#
        ));
        // 空响应
        assert!(!looks_like_sse_response(""));
    }
}
