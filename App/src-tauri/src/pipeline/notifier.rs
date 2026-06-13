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
        if is_in_cooldown(&state, &target, &event, &alarm) {
            continue;
        }
        let record = send_to_target(&mut target, &event, &payload, Some(&alarm)).await;
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

pub async fn send_test(
    app: AppHandle,
    state: Arc<AppState>,
    mut target: NotificationTarget,
) -> NotificationRecord {
    let now = chrono::Utc::now().timestamp_millis();
    let payload = serde_json::json!({
        "event": "test",
        "source_id": null,
        "source_name": "测试通道",
        "location": "EcoAlert",
        "person": false,
        "light": true,
        "alarm": true,
        "confidence": 1.0,
        "state_source": "test",
        "ts": now,
    });
    let record = send_to_target(&mut target, "test", &payload, None).await;
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

fn is_in_cooldown(
    state: &AppState,
    target: &NotificationTarget,
    event: &str,
    alarm: &AlarmRecord,
) -> bool {
    let cutoff = chrono::Utc::now().timestamp_millis() - (target.cooldown_sec as i64 * 1000);
    state
        .notification_history
        .lock()
        .records
        .iter()
        .any(|record| {
            record.ok
                && record.target_id == target.id
                && record.event == event
                && record.source_id.as_deref() == Some(alarm.source_id.as_str())
                && record.request_at >= cutoff
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
    serde_json::json!({
        "event": event,
        "alarm_id": alarm.id,
        "alarm_status": alarm.status,
        "source_id": alarm.source_id,
        "source_name": source.as_ref().map(|s| s.name.as_str()).unwrap_or(""),
        "location": source.as_ref().map(|s| s.location.as_str()).unwrap_or(""),
        "person": scene.as_ref().map(|s| s.person).unwrap_or(false),
        "light": scene.as_ref().map(|s| s.light).unwrap_or(false),
        "alarm": alarm.status == "alarm_active" || alarm.status == "acknowledged",
        "confidence": scene.as_ref().map(|s| s.confidence).unwrap_or(0.0),
        "state_source": "mock",
        "ts": chrono::Utc::now().timestamp_millis(),
    })
}

async fn send_to_target(
    target: &mut NotificationTarget,
    event: &str,
    payload: &Value,
    alarm: Option<&AlarmRecord>,
) -> NotificationRecord {
    let now = chrono::Utc::now().timestamp_millis();
    let mut record = NotificationRecord::new_pending(
        target,
        event.to_string(),
        alarm.map(|item| item.source_id.clone()),
        alarm.map(|item| item.id.clone()),
        None,
        now,
    );

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

    Ok(response.status().as_u16())
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
    Ok(response.status().as_u16())
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

fn event_label(event: &str) -> Cow<'_, str> {
    match event {
        "alarm_triggered" => Cow::Borrowed("报警触发"),
        "alarm_resolved" => Cow::Borrowed("报警恢复"),
        "test" => Cow::Borrowed("测试通知"),
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

    let alarm_icon = if alarm { "🚨" } else { "✅" };
    format!(
        "{icon} [EcoAlert] {event}\n通道: {source}\n位置: {location}\n有人: {person}\n亮灯: {light}\n时间: {time}",
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
