use crate::state::{log_event, AppState};
use crate::store::{
    AlarmRecord, NotificationRecord, NotificationTarget, NotificationTargetPayload,
};
use reqwest::Method;
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

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
    for target in targets {
        if !target_accepts_event(&target, &event) {
            continue;
        }
        if is_in_cooldown(&state, &target, &event, &alarm) {
            continue;
        }
        let record = send_to_target(&target, &event, &payload, Some(&alarm)).await;
        persist_and_emit(&app, &state, record);
    }
}

pub async fn send_test(
    app: AppHandle,
    state: Arc<AppState>,
    target: NotificationTarget,
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
    let record = send_to_target(&target, "test", &payload, None).await;
    persist_and_emit(&app, &state, record.clone());
    record
}

pub async fn resend_record(
    app: AppHandle,
    state: Arc<AppState>,
    target: NotificationTarget,
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
        let result = send_http(&target, body).await;
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
    }
    persist_and_emit(&app, &state, record.clone());
    record
}

pub fn target_from_payload(payload: NotificationTargetPayload) -> NotificationTarget {
    NotificationTarget::new(payload)
}

fn target_accepts_event(target: &NotificationTarget, event: &str) -> bool {
    target.enabled
        && !target.url.trim().is_empty()
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
    target: &NotificationTarget,
    event: &str,
    payload: &Value,
    alarm: Option<&AlarmRecord>,
) -> NotificationRecord {
    let now = chrono::Utc::now().timestamp_millis();
    let body = render_body(target, payload);
    let mut record = NotificationRecord::new_pending(
        target,
        event.to_string(),
        alarm.map(|item| item.source_id.clone()),
        alarm.map(|item| item.id.clone()),
        Some(body.clone()),
        now,
    );

    if target.url.starts_with("mock://") {
        record.ok = true;
        record.status_code = Some(200);
        record.latency_ms = Some(0);
        return record;
    }

    let started = Instant::now();
    let result = send_http(target, body).await;
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

async fn send_http(target: &NotificationTarget, body: String) -> Result<u16, String> {
    let method = target
        .method
        .parse::<Method>()
        .map_err(|_| "通知 Method 不合法".to_string())?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(target.timeout_sec as u64))
        .build()
        .map_err(|err| err.to_string())?;
    let mut request = client.request(method, target.url.trim()).body(body);
    for header in &target.headers {
        if !header.name.trim().is_empty() {
            request = request.header(header.name.trim(), header.value.as_str());
        }
    }
    let response = request.send().await.map_err(|err| err.to_string())?;
    Ok(response.status().as_u16())
}

fn render_body(target: &NotificationTarget, payload: &Value) -> String {
    let template = if target.body_template.trim().is_empty() {
        serde_json::to_string(payload).unwrap_or_else(|_| "{}".into())
    } else {
        target.body_template.clone()
    };
    let mut body = template;
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
