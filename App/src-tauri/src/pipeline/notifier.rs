use crate::state::{log_event, AppState};
use crate::store::{
    AlarmRecord, NotificationRecord, NotificationTarget, NotificationTargetPayload,
};
use reqwest::Method;
use serde_json::Value;
use std::borrow::Cow;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

/// 全局复用 HTTP Client，避免每次通知都重建连接池
static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn shared_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("初始化 HTTP Client 失败")
    })
}

pub async fn dispatch_alarm_event(
    app: AppHandle,
    state: Arc<AppState>,
    event: String,
    alarm: AlarmRecord,
) {
    let targets = state.notification_config.lock().targets.clone();
    if targets.is_empty() {
        return;
    }

    let payload = build_alarm_payload(&state, &event, &alarm);
    let mut updated_targets = Vec::new();
    for mut target in targets {
        if !target_accepts_event(&target, &event) {
            continue;
        }
        if is_in_cooldown_for_event(
            &state,
            &target,
            &event,
            Some(alarm.source_id.as_str()),
            Some(alarm.id.as_str()),
        ) {
            continue;
        }
        let record = send_to_target(
            &mut target,
            &event,
            &payload,
            Some(alarm.source_id.clone()),
            Some(alarm.id.clone()),
        )
        .await;
        // 如果 token 被刷新了，记录下来
        if target.is_api_mode() && !target.access_token.is_empty() {
            updated_targets.push(target.clone());
        }
        persist_and_emit(&app, &state, record);
    }
    // 持久化更新的 token
    if !updated_targets.is_empty() {
        let mut cfg = state.notification_config.lock();
        for updated in &updated_targets {
            if let Some(t) = cfg.targets.iter_mut().find(|t| t.id == updated.id) {
                t.access_token = updated.access_token.clone();
                t.token_expires_at = updated.token_expires_at;
            }
        }
    }
}

pub async fn dispatch_system_event(
    app: AppHandle,
    state: Arc<AppState>,
    event: String,
    source_id: Option<String>,
    title: String,
    message: String,
) {
    let targets = state.notification_config.lock().targets.clone();
    if targets.is_empty() {
        return;
    }

    let payload = build_system_payload(&state, &event, source_id.as_deref(), &title, &message);
    let mut updated_targets = Vec::new();
    for mut target in targets {
        if !target_accepts_event(&target, &event) {
            continue;
        }
        if is_in_cooldown_for_event(&state, &target, &event, source_id.as_deref(), None) {
            continue;
        }
        let record = send_to_target(&mut target, &event, &payload, source_id.clone(), None).await;
        if target.is_api_mode() && !target.access_token.is_empty() {
            updated_targets.push(target.clone());
        }
        persist_and_emit(&app, &state, record);
    }
    if !updated_targets.is_empty() {
        let mut cfg = state.notification_config.lock();
        for updated in &updated_targets {
            if let Some(t) = cfg.targets.iter_mut().find(|t| t.id == updated.id) {
                t.access_token = updated.access_token.clone();
                t.token_expires_at = updated.token_expires_at;
            }
        }
    }
}

pub async fn send_test(
    app: AppHandle,
    state: Arc<AppState>,
    mut target: NotificationTarget,
) -> NotificationRecord {
    let now = chrono::Utc::now().timestamp_millis();
    let ts_formatted = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let payload = serde_json::json!({
        "event": "test",
        "source_id": null,
        "source_name": "测试视频源",
        "location": "测试区域",
        "source_url": "",
        "person": false,
        "light": true,
        "alarm": true,
        "confidence": 1.0,
        "state_source": "test",
        "ts": now,
        "ts_formatted": ts_formatted,
    });
    let record = send_to_target(&mut target, "test", &payload, None, None).await;
    persist_and_emit(&app, &state, record.clone());
    // 如果 token 被刷新了，持久化
    if target.is_api_mode() && !target.access_token.is_empty() {
        let mut cfg = state.notification_config.lock();
        if let Some(t) = cfg.targets.iter_mut().find(|t| t.id == target.id) {
            t.access_token = target.access_token.clone();
            t.token_expires_at = target.token_expires_at;
        }
    }
    record
}

pub async fn resend_record(
    app: AppHandle,
    state: Arc<AppState>,
    mut target: NotificationTarget,
    original: NotificationRecord,
) -> NotificationRecord {
    let body = original.request_body.clone().unwrap_or_else(|| "{}".into());
    let now = chrono::Utc::now().timestamp_millis();
    let mut record = NotificationRecord::new_pending(
        &target,
        original.event.clone(),
        original.source_id.clone(),
        original.alarm_id.clone(),
        Some(body.clone()),
        now,
    );
    record.retry_count = original.retry_count.saturating_add(1);

    if target.url.starts_with("mock://") {
        record.ok = true;
        record.status_code = Some(200);
        record.latency_ms = Some(0);
    } else {
        let started = Instant::now();
        let result = if target.is_api_mode() {
            send_platform_message(&mut target, &body).await
        } else {
            send_http(&target, body).await
        };
        record.latency_ms = Some(started.elapsed().as_millis().min(u32::MAX as u128) as u32);
        match result {
            Ok(status) => {
                record.ok = (200..300).contains(&status);
                record.status_code = Some(status);
                if !record.ok {
                    record.error = Some(format!("HTTP {status}"));
                }
            }
            Err(err) => {
                record.ok = false;
                record.error = Some(err);
            }
        }
        // 持久化 token
        if target.is_api_mode() && !target.access_token.is_empty() {
            let mut cfg = state.notification_config.lock();
            if let Some(t) = cfg.targets.iter_mut().find(|t| t.id == target.id) {
                t.access_token = target.access_token.clone();
                t.token_expires_at = target.token_expires_at;
            }
        }
    }
    persist_and_emit(&app, &state, record.clone());
    record
}

pub fn target_from_payload(payload: NotificationTargetPayload) -> NotificationTarget {
    NotificationTarget::new(payload)
}

fn target_accepts_event(target: &NotificationTarget, event: &str) -> bool {
    target.enabled
        && (target.is_api_mode() || !target.url.trim().is_empty())
        && (target.event_types.is_empty() || target.event_types.iter().any(|item| item == event))
}

fn is_in_cooldown_for_event(
    state: &AppState,
    target: &NotificationTarget,
    event: &str,
    source_id: Option<&str>,
    alarm_id: Option<&str>,
) -> bool {
    let cutoff = chrono::Utc::now().timestamp_millis() - (target.cooldown_sec as i64 * 1000);
    state
        .notification_history
        .lock()
        .records
        .iter()
        .any(|record| cooldown_record_matches(record, target, event, source_id, alarm_id, cutoff))
}

fn cooldown_record_matches(
    record: &NotificationRecord,
    target: &NotificationTarget,
    event: &str,
    source_id: Option<&str>,
    alarm_id: Option<&str>,
    cutoff: i64,
) -> bool {
    record.ok
        && record.target_id == target.id
        && record.event == event
        && match alarm_id {
            // 每个报警生命周期都必须发送一次，不能被同一视频源的上一条报警抑制。
            Some(id) => record.alarm_id.as_deref() == Some(id),
            None => record.source_id.as_deref() == source_id,
        }
        && record.request_at >= cutoff
}

fn build_system_payload(
    state: &AppState,
    event: &str,
    source_id: Option<&str>,
    title: &str,
    message: &str,
) -> Value {
    let source = source_id.and_then(|id| {
        state
            .sources
            .lock()
            .iter()
            .find(|source| source.id == id)
            .cloned()
    });
    let now = chrono::Utc::now();
    let ts_formatted = now
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    serde_json::json!({
        "event": event,
        "source_id": source_id,
        "source_name": source.as_ref().map(|s| s.name.as_str()).unwrap_or("YOLO服务器"),
        "location": source.as_ref().map(|s| s.location.as_str()).unwrap_or(""),
        "source_url": source.as_ref().map(|s| s.url.as_str()).unwrap_or(""),
        "title": title,
        "message": message,
        "state_source": "yolo_error",
        "ts": now.timestamp_millis(),
        "ts_formatted": ts_formatted,
    })
}

fn build_alarm_payload(state: &AppState, event: &str, alarm: &AlarmRecord) -> Value {
    let source = state
        .sources
        .lock()
        .iter()
        .find(|source| source.id == alarm.source_id)
        .cloned();
    let scene = state.current_state.lock().get(&alarm.source_id).cloned();
    let now = chrono::Utc::now();
    let ts_formatted = now
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    serde_json::json!({
        "event": event,
        "alarm_id": alarm.id,
        "alarm_status": alarm.status,
        "source_id": alarm.source_id,
        "source_name": source.as_ref().map(|s| s.name.as_str()).unwrap_or(""),
        "location": source.as_ref().map(|s| s.location.as_str()).unwrap_or(""),
        "source_url": source.as_ref().map(|s| s.url.as_str()).unwrap_or(""),
        "person": scene.as_ref().map(|s| s.person).unwrap_or(false),
        "light": scene.as_ref().map(|s| s.light).unwrap_or(false),
        "alarm": alarm.status == "alarm_active" || alarm.status == "acknowledged",
        "confidence": scene.as_ref().map(|s| s.confidence).unwrap_or(0.0),
        "state_source": "mock",
        "ts": now.timestamp_millis(),
        "ts_formatted": ts_formatted,
    })
}

async fn send_to_target(
    target: &mut NotificationTarget,
    event: &str,
    payload: &Value,
    source_id: Option<String>,
    alarm_id: Option<String>,
) -> NotificationRecord {
    let now = chrono::Utc::now().timestamp_millis();
    let mut record =
        NotificationRecord::new_pending(target, event.to_string(), source_id, alarm_id, None, now);

    if target.url.starts_with("mock://") {
        record.ok = true;
        record.status_code = Some(200);
        record.latency_ms = Some(0);
        return record;
    }

    let started = Instant::now();

    // API 凭证模式 vs Webhook 模式
    let result = if target.is_api_mode() {
        let body = render_body(target, payload);
        record.request_body = Some(body.clone());
        send_platform_message(target, &body).await
    } else {
        let body = render_body(target, payload);
        record.request_body = Some(body.clone());
        send_http(target, body).await
    };

    record.latency_ms = Some(started.elapsed().as_millis().min(u32::MAX as u128) as u32);
    match result {
        Ok(status) => {
            record.ok = (200..300).contains(&status);
            record.status_code = Some(status);
            if !record.ok {
                record.error = Some(format!("HTTP {status}"));
            }
        }
        Err(err) => {
            record.ok = false;
            record.error = Some(err);
        }
    }
    record
}

/// API 模式：用平台官方 API 发送（自动获取 token）
async fn send_platform_message(target: &mut NotificationTarget, body: &str) -> Result<u16, String> {
    let token = super::channel_auth::ensure_access_token(target).await?;
    let client = shared_client();
    let timeout = std::time::Duration::from_secs(target.timeout_sec as u64);

    let response = match target.channel_type.as_str() {
        "feishu" => {
            let url =
                format!("https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type=chat_id");
            client
                .post(&url)
                .timeout(timeout)
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json; charset=utf-8")
                .body(body.to_string())
                .send()
                .await
                .map_err(|e| format!("飞书 API 请求失败: {e}"))?
        }
        "wechat_work" => {
            let url =
                format!("https://qyapi.weixin.qq.com/cgi-bin/message/send?access_token={token}");
            client
                .post(&url)
                .timeout(timeout)
                .header("Content-Type", "application/json")
                .body(body.to_string())
                .send()
                .await
                .map_err(|e| format!("企微 API 请求失败: {e}"))?
        }
        "qqbot" => {
            if target.chat_id.trim().is_empty() {
                return Ok(204);
            }
            let url = format!(
                "https://api.sgroup.qq.com/v2/groups/{}/messages",
                target.chat_id
            );
            client
                .post(&url)
                .timeout(timeout)
                .header("Authorization", format!("QQBot {token}"))
                .header("Content-Type", "application/json")
                .body(body.to_string())
                .send()
                .await
                .map_err(|e| format!("QQ API 请求失败: {e}"))?
        }
        other => return Err(format!("不支持的渠道类型: {other}")),
    };

    validate_http_response(response, "平台通知").await
}

async fn send_http(target: &NotificationTarget, body: String) -> Result<u16, String> {
    let method = target
        .method
        .parse::<Method>()
        .map_err(|_| "通知 Method 不合法".to_string())?;
    let client = shared_client();
    let mut request = client
        .request(method, target.url.trim())
        .timeout(std::time::Duration::from_secs(target.timeout_sec as u64))
        .body(body);
    for header in &target.headers {
        if !header.name.trim().is_empty() {
            request = request.header(header.name.trim(), header.value.as_str());
        }
    }
    let response = request.send().await.map_err(|err| err.to_string())?;
    validate_http_response(response, "Webhook").await
}

async fn validate_http_response(response: reqwest::Response, channel: &str) -> Result<u16, String> {
    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|err| format!("读取{channel}响应失败: {err}"))?;
    if let Ok(json) = serde_json::from_str::<Value>(&text) {
        if business_response_failed(&json) {
            return Err(format!(
                "{channel}返回 HTTP {status}，但业务结果失败: {}",
                text_snippet(&text, 500)
            ));
        }
    }
    Ok(status)
}

fn business_response_failed(json: &Value) -> bool {
    json.get("success")
        .and_then(Value::as_bool)
        .map(|value| !value)
        .unwrap_or(false)
        || json
            .get("errcode")
            .map(|value| value.as_i64() != Some(0) && value.as_str() != Some("0"))
            .unwrap_or(false)
        || json
            .get("code")
            .map(|value| {
                !matches!(value.as_i64(), Some(0 | 200))
                    && !matches!(value.as_str(), Some("0" | "200"))
            })
            .unwrap_or(false)
}

fn text_snippet(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let snippet = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{snippet}...")
    } else {
        snippet
    }
}

fn render_body(target: &NotificationTarget, payload: &Value) -> String {
    // 有自定义模板就用模板（向后兼容）
    if !target.body_template.trim().is_empty() {
        return apply_template(&target.body_template, payload);
    }
    // API 凭证模式 vs Webhook 模式
    if target.is_api_mode() {
        match target.channel_type.as_str() {
            "feishu" => render_feishu_api(&target.chat_id, payload),
            "wechat_work" => render_wechat_work_api(&target.chat_id, &target.agent_id, payload),
            "qqbot" => render_qqbot(payload), // QQ 的 group_openid 在 URL 里
            _ => serde_json::to_string(payload).unwrap_or_else(|_| "{}".into()),
        }
    } else {
        // Webhook 模式
        match target.channel_type.as_str() {
            "feishu" => render_feishu_webhook(payload),
            "wechat_work" => render_wechat_work_webhook(payload),
            "qqbot" => render_qqbot(payload),
            _ => serde_json::to_string(payload).unwrap_or_else(|_| "{}".into()),
        }
    }
}

fn apply_template(template: &str, payload: &Value) -> String {
    // JSON 模板必须在结构化值中替换，否则 Windows 路径中的反斜杠、换行和
    // 引号会破坏请求体。测试通知没有这些字符，因此旧实现只在真实报警暴露。
    if let Ok(mut json) = serde_json::from_str::<Value>(template) {
        render_json_template_value(&mut json, payload);
        return serde_json::to_string(&json).unwrap_or_else(|_| template.to_string());
    }

    let mut body = template.to_string();
    if let Some(obj) = payload.as_object() {
        for (key, value) in obj {
            let value = value
                .as_str()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| value.to_string());
            body = body.replace(&format!("{{{{{key}}}}}"), &value);
        }
    }
    body
}

fn render_json_template_value(value: &mut Value, payload: &Value) {
    match value {
        Value::String(text) => *text = apply_text_template(text, payload),
        Value::Array(items) => {
            for item in items {
                render_json_template_value(item, payload);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                render_json_template_value(item, payload);
            }
        }
        _ => {}
    }
}

fn apply_text_template(template: &str, payload: &Value) -> String {
    let mut text = template.to_string();
    if let Some(obj) = payload.as_object() {
        for (key, value) in obj {
            let replacement = value
                .as_str()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| value.to_string());
            text = text.replace(&format!("{{{{{key}}}}}"), &replacement);
        }
    }
    text
}

fn event_label(event: &str) -> Cow<'_, str> {
    match event {
        "alarm_triggered" => Cow::Borrowed("报警触发"),
        "alarm_resolved" => Cow::Borrowed("报警恢复"),
        "test" => Cow::Borrowed("测试通知"),
        "yolo_error" => Cow::Borrowed("YOLO 服务器失效"),
        _ => Cow::Borrowed(event),
    }
}

fn format_message(payload: &Value) -> String {
    let event = payload["event"].as_str().unwrap_or("unknown");
    let source = payload["source_name"].as_str().unwrap_or("-");
    let location = payload["location"].as_str().unwrap_or("-");
    let alarm = payload["alarm"].as_bool().unwrap_or(false);
    let person = payload["person"].as_bool().unwrap_or(false);
    let light = payload["light"].as_bool().unwrap_or(false);
    let ts = payload["ts"].as_i64().unwrap_or(0);
    let explicit_title = payload["title"].as_str();
    let explicit_message = payload["message"].as_str();

    let time_str = if ts > 0 {
        chrono::DateTime::from_timestamp_millis(ts)
            .map(|dt| {
                dt.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|| ts.to_string())
    } else {
        "-".into()
    };

    if let Some(message) = explicit_message {
        let title = explicit_title
            .map(Cow::Borrowed)
            .unwrap_or_else(|| event_label(event));
        return format!(
            "⚠️ [EcoAlert] {title}\n视频源: {source}\n区域: {location}\n详情: {message}\n时间: {time}",
            title = title,
            source = source,
            location = location,
            message = message,
            time = time_str,
        );
    }

    let alarm_icon = if alarm { "🚨" } else { "✅" };
    format!(
        "{icon} [EcoAlert] {event}\n视频源: {source}\n区域: {location}\n有人: {person}\n亮灯: {light}\n时间: {time}",
        icon = alarm_icon,
        event = event_label(event),
        source = source,
        location = location,
        person = if person { "是" } else { "否" },
        light = if light { "是" } else { "否" },
        time = time_str,
    )
}

// ---- API 模式（含 chat_id 等目标字段）----

fn render_feishu_api(chat_id: &str, payload: &Value) -> String {
    let text = format_message(payload);
    // content 字段需要是 JSON 字符串
    let content = serde_json::json!({ "text": text }).to_string();
    serde_json::json!({
        "receive_id": chat_id,
        "msg_type": "text",
        "content": content
    })
    .to_string()
}

fn render_wechat_work_api(touser: &str, agent_id: &str, payload: &Value) -> String {
    let text = format_message(payload);
    let agent_id_num: u64 = agent_id.parse().unwrap_or(0);
    serde_json::json!({
        "touser": touser,
        "msgtype": "text",
        "agentid": agent_id_num,
        "text": { "content": text }
    })
    .to_string()
}

// ---- Webhook 模式（原始报文）----

fn render_feishu_webhook(payload: &Value) -> String {
    let text = format_message(payload);
    serde_json::json!({
        "msg_type": "text",
        "content": { "text": text }
    })
    .to_string()
}

fn render_wechat_work_webhook(payload: &Value) -> String {
    let text = format_message(payload);
    serde_json::json!({
        "msgtype": "text",
        "text": { "content": text }
    })
    .to_string()
}

fn render_qqbot(payload: &Value) -> String {
    let text = format_message(payload);
    serde_json::json!({
        "msg_type": 0,
        "content": text
    })
    .to_string()
}

fn persist_and_emit(app: &AppHandle, state: &AppState, record: NotificationRecord) {
    if let Err(err) = state.record_notification(record.clone()) {
        log_event(app, "warn", format!("通知历史落库失败: {err}"));
    }
    let _ = app.emit(
        "ecoalert://notification",
        serde_json::json!({
            "record_id": record.id,
            "target_id": record.target_id,
            "event": record.event,
            "ok": record.ok,
            "status": record.status_code,
            "error": record.error,
            "ts": record.request_at,
        }),
    );
    if record.ok {
        log_event(app, "info", format!("通知发送成功: {}", record.target_name));
    } else {
        log_event(
            app,
            "warn",
            format!(
                "通知发送失败: {} ({})",
                record.target_name,
                record.error.as_deref().unwrap_or("unknown")
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_target() -> NotificationTarget {
        NotificationTarget::new(NotificationTargetPayload {
            name: "target".into(),
            enabled: true,
            channel_type: "webhook".into(),
            url: "mock://local".into(),
            ..NotificationTargetPayload::default()
        })
    }

    #[test]
    fn cooldown_does_not_suppress_a_new_alarm_for_the_same_source() {
        let target = test_target();
        let record = NotificationRecord::new_pending(
            &target,
            "alarm_triggered".into(),
            Some("camera-1".into()),
            Some("alarm-old".into()),
            None,
            10_000,
        );
        let record = NotificationRecord { ok: true, ..record };

        assert!(cooldown_record_matches(
            &record,
            &target,
            "alarm_triggered",
            Some("camera-1"),
            Some("alarm-old"),
            0,
        ));
        assert!(!cooldown_record_matches(
            &record,
            &target,
            "alarm_triggered",
            Some("camera-1"),
            Some("alarm-new"),
            0,
        ));
    }

    #[test]
    fn json_template_escapes_windows_paths_and_newlines() {
        let template = r#"{"text":{"content":"区域: {{location}}\n来源: {{source_name}}"}}"#;
        let rendered = apply_template(
            template,
            &serde_json::json!({
                "location": r"G:\project\EcoAlert\Video",
                "source_name": "一号\n摄像头",
            }),
        );
        let parsed: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(
            parsed["text"]["content"],
            "区域: G:\\project\\EcoAlert\\Video\n来源: 一号\n摄像头"
        );
    }

    #[test]
    fn http_200_business_error_is_recognized() {
        let body = serde_json::json!({"errcode": 40003, "errmsg": "invalid user"});
        assert!(business_response_failed(&body));
        assert!(!business_response_failed(
            &serde_json::json!({"errcode": 0})
        ));
        assert!(!business_response_failed(&serde_json::json!({"code": 200})));
    }

    #[test]
    fn system_message_uses_explicit_error_detail() {
        let payload = serde_json::json!({
            "event": "yolo_error",
            "source_name": "camera-1",
            "location": "office",
            "title": "YOLO 服务器失效",
            "message": "连接超时",
            "ts": 1_700_000_000_000_i64,
        });
        let message = format_message(&payload);
        assert!(message.contains("YOLO 服务器失效"));
        assert!(message.contains("连接超时"));
        assert!(message.contains("camera-1"));
    }
}
