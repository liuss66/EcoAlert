use crate::pipeline::decoder::DecodedFrame;
use crate::store::AlgorithmConfig;
use anyhow::{anyhow, Context};
use image::codecs::jpeg::JpegEncoder;
use image::{imageops::FilterType, DynamicImage, ImageBuffer, Rgb};
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
struct ChatMessage {
    content: String,
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
    let body = serde_json::json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "image_url", "image_url": { "url": image_url } },
                { "type": "text", "text": config.vlm_prompt },
            ],
        }],
        "max_tokens": config.vlm_max_tokens.max(16),
        "temperature": config.vlm_temperature.clamp(0.0, 2.0),
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let url = format!("{api_base}/chat/completions");
    let response = client
        .post(url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .context("VLM 请求发送失败")?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("VLM 请求失败 HTTP {status}: {text}");
    }

    let parsed: ChatCompletionResponse =
        serde_json::from_str(&text).context("VLM 响应不是 OpenAI-compatible JSON")?;
    let content = parsed
        .choices
        .first()
        .map(|choice| choice.message.content.trim().to_string())
        .ok_or_else(|| anyhow!("VLM 响应缺少 choices[0].message.content"))?;
    parse_detection_content(&content)
}

pub async fn test_connection(
    config: &AlgorithmConfig,
) -> anyhow::Result<(String, Option<VlmUsage>)> {
    let body = serde_json::json!({
        "model": config.vlm_model.trim(),
        "messages": [{ "role": "user", "content": "Hi" }],
        "max_tokens": 16,
        "temperature": 0.0,
    });
    let parsed = call_chat_completion(config, body).await?;
    let content = parsed
        .choices
        .first()
        .map(|choice| choice.message.content.trim().to_string())
        .unwrap_or_default();
    Ok((content, parsed.usage.map(Into::into)))
}

async fn call_chat_completion(
    config: &AlgorithmConfig,
    body: serde_json::Value,
) -> anyhow::Result<ChatCompletionResponse> {
    let api_base = config.vlm_api_base.trim().trim_end_matches('/');
    let api_key = config.vlm_api_key.trim();
    let model = config.vlm_model.trim();
    if api_base.is_empty() || api_key.is_empty() || model.is_empty() {
        anyhow::bail!("VLM API 地址、API Key、模型名称不能为空");
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let url = format!("{api_base}/chat/completions");
    let response = client
        .post(url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .context("VLM 请求发送失败")?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("VLM 请求失败 HTTP {status}: {text}");
    }
    serde_json::from_str(&text).context("VLM 响应不是 OpenAI-compatible JSON")
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
}
