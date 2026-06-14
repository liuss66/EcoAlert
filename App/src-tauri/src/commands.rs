// 暴露给前端的 Tauri commands
use crate::pipeline::{
    decoder::extract_gray_frame_from_url, detector::Detector, notifier, scheduler, vlm,
    PipelineConfig,
};
use crate::state::{log_event, AppState};
use crate::store::{
    records_by_source, AlarmRecord, AlgorithmConfig, ChannelRuntimeStatus, DetectionSampleRecord,
    NotificationRecord, NotificationTarget, NotificationTargetPayload, RoiConfig, SceneState,
    SecurityConfig, SourceGroup, StateRecord, VideoSource, GLOBAL_ROI_SOURCE_ID,
};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

fn require_login(state: &State<Arc<AppState>>) -> Result<(), String> {
    if !*state.logged_in.lock() {
        return Err("未登录或会话已过期".into());
    }
    Ok(())
}

#[tauri::command]
pub fn login(
    app: AppHandle,
    state: State<Arc<AppState>>,
    password: String,
) -> Result<serde_json::Value, String> {
    let ok = state.auth.lock().verify(&password);
    if !ok {
        log_event(&app, "warn", "登录失败：密码错误");
        return Err("密码错误".into());
    }
    *state.logged_in.lock() = true;
    log_event(&app, "info", "管理员登录成功");
    Ok(serde_json::json!({ "ok": true, "token": "tauri-session" }))
}

#[tauri::command]
pub fn logout(state: State<Arc<AppState>>) -> Result<serde_json::Value, String> {
    *state.logged_in.lock() = false;
    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub fn check_auth(state: State<Arc<AppState>>) -> Result<serde_json::Value, String> {
    if *state.logged_in.lock() {
        Ok(serde_json::json!({ "ok": true }))
    } else {
        Err("未登录".into())
    }
}

/* -------------------- 视频源 -------------------- */

#[tauri::command]
pub fn list_sources(state: State<Arc<AppState>>) -> Result<Vec<VideoSource>, String> {
    require_login(&state)?;
    Ok(state.sources.lock().clone())
}

#[tauri::command]
pub fn list_groups(state: State<Arc<AppState>>) -> Result<Vec<SourceGroup>, String> {
    require_login(&state)?;
    let mut gs = state.groups.lock().clone();
    gs.sort_by_key(|g| g.order);
    Ok(gs)
}

#[derive(serde::Deserialize)]
pub struct SourcePayload {
    pub name: String,
    pub url: String,
    #[serde(rename = "type")]
    pub source_type: String,
    pub location: String,
    pub enabled: bool,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub order: Option<i32>,
}

fn validate_type(t: &str) -> bool {
    matches!(t, "hls" | "mp4" | "webcam" | "rtsp")
}

#[tauri::command]
pub fn create_source(
    app: AppHandle,
    state: State<Arc<AppState>>,
    payload: SourcePayload,
) -> Result<VideoSource, String> {
    require_login(&state)?;
    if payload.name.trim().is_empty() || payload.url.trim().is_empty() {
        return Err("名称和地址不能为空".into());
    }
    let ty = if validate_type(&payload.source_type) {
        payload.source_type.clone()
    } else {
        "hls".into()
    };
    // 校验 group_id
    let group_id = payload
        .group_id
        .clone()
        .filter(|g| state.groups.lock().iter().any(|x| &x.id == g));
    let order = payload.order.unwrap_or(0);
    let item = VideoSource::new(
        payload.name.chars().take(64).collect(),
        payload.url.chars().take(512).collect(),
        ty,
        payload.location.chars().take(128).collect(),
        payload.enabled,
        group_id,
        order,
    );
    {
        let mut sources = state.sources.lock();
        sources.push(item.clone());
    }
    state.persist_sources().map_err(|e| e.to_string())?;
    log_event(
        &app,
        "info",
        format!("新增视频源: {} ({})", item.name, item.url),
    );
    Ok(item)
}

#[tauri::command]
pub fn update_source(
    app: AppHandle,
    state: State<Arc<AppState>>,
    id: String,
    payload: SourcePayload,
) -> Result<VideoSource, String> {
    require_login(&state)?;
    let mut sources = state.sources.lock();
    let idx = sources
        .iter()
        .position(|s| s.id == id)
        .ok_or("视频源不存在")?;
    let cur = sources[idx].clone();
    let ty = if validate_type(&payload.source_type) {
        payload.source_type.clone()
    } else {
        cur.source_type.clone()
    };
    let group_id = payload
        .group_id
        .clone()
        .or(cur.group_id.clone())
        .filter(|g| state.groups.lock().iter().any(|x| &x.id == g));
    let updated = VideoSource {
        id: cur.id.clone(),
        name: payload.name.chars().take(64).collect(),
        url: payload.url.chars().take(512).collect(),
        source_type: ty,
        location: payload.location.chars().take(128).collect(),
        enabled: payload.enabled,
        group_id,
        order: payload.order.unwrap_or(cur.order),
        created_at: cur.created_at,
    };
    sources[idx] = updated.clone();
    drop(sources);
    state.persist_sources().map_err(|e| e.to_string())?;
    log_event(&app, "info", format!("更新视频源: {}", updated.name));
    Ok(updated)
}

#[tauri::command]
pub fn delete_source(
    app: AppHandle,
    state: State<Arc<AppState>>,
    id: String,
) -> Result<serde_json::Value, String> {
    require_login(&state)?;
    let removed = {
        let mut sources = state.sources.lock();
        let idx = sources
            .iter()
            .position(|s| s.id == id)
            .ok_or("视频源不存在")?;
        Some(sources.remove(idx))
    };
    if let Some(r) = removed {
        state.persist_sources().map_err(|e| e.to_string())?;
        log_event(&app, "info", format!("删除视频源: {}", r.name));
    }
    Ok(serde_json::json!({ "ok": true }))
}

/* -------------------- 分组 -------------------- */

#[derive(serde::Deserialize)]
pub struct GroupPayload {
    pub name: String,
    #[serde(default)]
    pub order: i32,
    #[serde(default)]
    pub collapsed: bool,
}

#[tauri::command]
pub fn create_group(
    app: AppHandle,
    state: State<Arc<AppState>>,
    payload: GroupPayload,
) -> Result<SourceGroup, String> {
    require_login(&state)?;
    if payload.name.trim().is_empty() {
        return Err("分组名不能为空".into());
    }
    let grp = SourceGroup::new(
        payload.name.chars().take(64).collect::<String>(),
        payload.order,
    );
    let mut grp = grp;
    grp.collapsed = payload.collapsed;
    {
        let mut gs = state.groups.lock();
        gs.push(grp.clone());
    }
    state.persist_sources().map_err(|e| e.to_string())?;
    log_event(&app, "info", format!("新增分组: {}", grp.name));
    Ok(grp)
}

#[tauri::command]
pub fn update_group(
    app: AppHandle,
    state: State<Arc<AppState>>,
    id: String,
    payload: GroupPayload,
) -> Result<SourceGroup, String> {
    require_login(&state)?;
    let mut gs = state.groups.lock();
    let idx = gs.iter().position(|g| g.id == id).ok_or("分组不存在")?;
    let cur = gs[idx].clone();
    let updated = SourceGroup {
        id: cur.id.clone(),
        name: payload.name.chars().take(64).collect(),
        order: payload.order,
        collapsed: payload.collapsed,
        created_at: cur.created_at,
    };
    gs[idx] = updated.clone();
    drop(gs);
    state.persist_sources().map_err(|e| e.to_string())?;
    log_event(&app, "info", format!("更新分组: {}", updated.name));
    Ok(updated)
}

#[tauri::command]
pub fn delete_group(
    app: AppHandle,
    state: State<Arc<AppState>>,
    id: String,
) -> Result<serde_json::Value, String> {
    require_login(&state)?;
    // 不允许删除默认分组
    if id == "grp-default" {
        return Err("默认分组不能删除".into());
    }
    {
        let mut gs = state.groups.lock();
        let idx = gs.iter().position(|g| g.id == id).ok_or("分组不存在")?;
        gs.remove(idx);
    }
    // 属于该分组的源全部移到默认分组
    {
        let mut sources = state.sources.lock();
        for s in sources.iter_mut() {
            if s.group_id.as_deref() == Some(&id) {
                s.group_id = Some("grp-default".to_string());
            }
        }
    }
    state.persist_sources().map_err(|e| e.to_string())?;
    log_event(&app, "info", "删除分组");
    Ok(serde_json::json!({ "ok": true }))
}

/// 拖拽后批量更新顺序：传入 `[{id, order, group_id?}]`
#[derive(serde::Deserialize)]
pub struct OrderItem {
    pub id: String,
    pub order: i32,
    #[serde(default)]
    pub group_id: Option<String>,
}

#[tauri::command]
pub fn reorder(
    app: AppHandle,
    state: State<Arc<AppState>>,
    items: Vec<OrderItem>,
) -> Result<serde_json::Value, String> {
    require_login(&state)?;
    let count = items.len();
    {
        let mut sources = state.sources.lock();
        for it in items {
            if let Some(s) = sources.iter_mut().find(|s| s.id == it.id) {
                s.order = it.order;
                if let Some(g) = it.group_id {
                    s.group_id = Some(g);
                }
            }
        }
    }
    state.persist_sources().map_err(|e| e.to_string())?;
    log_event(&app, "debug", format!("重排 {} 项", count));
    Ok(serde_json::json!({ "ok": true }))
}

/* -------------------- 状态记录（算法输出） -------------------- */

/// 接收算法真实输出（生产中由 pipeline::Pipeline 调用）
/// 这里同时 emit 给前端 + 落库
#[tauri::command]
pub fn report_scene_state(
    app: AppHandle,
    state: State<Arc<AppState>>,
    source_id: String,
    person: bool,
    light: bool,
) -> Result<serde_json::Value, String> {
    require_login(&state)?;
    let scene = SceneState {
        person,
        light,
        frame_seq: 0,
        confidence: 1.0,
        source: "mock".into(),
        person_confidence: 1.0,
        light_confidence: 1.0,
        reason: None,
        model_latency_ms: None,
        light_brightness: if light { 255.0 } else { 0.0 },
        color_score: if light { 1.0 } else { 0.0 },
        motion_score: if person { 1.0 } else { 0.0 },
        process_ms: 0.0,
    };
    let rec = StateRecord::from_change(&source_id, &scene);
    state.record_state_change(rec).map_err(|e| e.to_string())?;
    state
        .record_detection_sample(DetectionSampleRecord::from_scene(
            &source_id,
            &scene,
            if !person && light {
                "suspected"
            } else {
                "normal"
            },
            chrono::Utc::now().timestamp_millis(),
        ))
        .map_err(|e| e.to_string())?;
    let _ = app.emit(
        "ecoalert://scene_state",
        serde_json::json!({
            "source_id": source_id,
            "person": person,
            "light": light,
            "light_state": if light { "on" } else { "off" },
            "ts": chrono::Utc::now().timestamp_millis(),
        }),
    );
    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub fn list_detection_history(
    state: State<Arc<AppState>>,
    source_id: Option<String>,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    require_login(&state)?;
    let limit = limit.unwrap_or(500).clamp(1, 5000);
    let h = state.detection_history.lock();
    let mut out: Vec<DetectionSampleRecord> = h
        .records
        .iter()
        .rev()
        .filter(|r| source_id.as_deref().map_or(true, |id| r.source_id == id))
        .take(limit)
        .cloned()
        .collect();
    out.reverse();
    Ok(serde_json::json!({
        "ok": true,
        "records": out,
    }))
}

/// 查询历史：每个源最近 N 条
#[tauri::command]
pub fn get_state_history(
    state: State<Arc<AppState>>,
    source_id: Option<String>,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    require_login(&state)?;
    let limit = limit.unwrap_or(100);
    let h = state.history.lock();
    let mut out: Vec<StateRecord> = h
        .records
        .iter()
        .filter(|r| source_id.as_deref().map_or(true, |id| r.source_id == id))
        .cloned()
        .collect();
    // 倒序取最近 N
    out.reverse();
    out.truncate(limit);
    let by_src = records_by_source(&out);
    Ok(serde_json::json!({
        "ok": true,
        "records": out,
        "by_source": by_src,
    }))
}

#[tauri::command]
pub fn get_channel_runtime_status(
    state: State<Arc<AppState>>,
    source_id: Option<String>,
) -> Result<Vec<ChannelRuntimeStatus>, String> {
    require_login(&state)?;
    let mut snapshot = state.runtime_status_snapshot();
    if let Some(id) = source_id {
        snapshot.retain(|item| item.source_id == id);
    }
    Ok(snapshot)
}

#[tauri::command]
pub fn list_alarms(
    state: State<Arc<AppState>>,
    status: Option<String>,
    source_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<AlarmRecord>, String> {
    require_login(&state)?;
    let mut records: Vec<AlarmRecord> = state
        .alarm_records
        .lock()
        .records
        .iter()
        .filter(|record| status.as_deref().map_or(true, |s| record.status == s))
        .filter(|record| {
            source_id
                .as_deref()
                .map_or(true, |id| record.source_id == id)
        })
        .cloned()
        .collect();
    records.reverse();
    records.truncate(limit.unwrap_or(100));
    Ok(records)
}

#[tauri::command]
pub fn ack_alarm(
    app: AppHandle,
    state: State<Arc<AppState>>,
    alarm_id: String,
    note: Option<String>,
) -> Result<AlarmRecord, String> {
    require_login(&state)?;
    let now = chrono::Utc::now().timestamp_millis();
    let updated = {
        let mut file = state.alarm_records.lock();
        let record = file
            .records
            .iter_mut()
            .find(|record| record.id == alarm_id)
            .ok_or("报警记录不存在")?;
        record.status = "acknowledged".into();
        record.acknowledged_at = Some(now);
        record.acknowledged_by = Some("admin".into());
        record.note = note;
        record.clone()
    };
    state.persist_alarm_records().map_err(|e| e.to_string())?;
    {
        let mut runtime = state.runtime_status.lock();
        if let Some(entry) = runtime.get_mut(&updated.source_id) {
            entry.alarm_status = updated.status.clone();
            entry.ts = now;
        }
    }
    let _ = app.emit(
        "ecoalert://alarm",
        serde_json::json!({
            "alarm_id": updated.id,
            "source_id": updated.source_id,
            "status": updated.status,
            "event": "alarm_acknowledged",
            "ts": now,
        }),
    );
    log_event(&app, "info", "报警已确认");
    Ok(updated)
}

#[tauri::command]
pub fn resolve_alarm(
    app: AppHandle,
    state: State<Arc<AppState>>,
    alarm_id: String,
    note: Option<String>,
) -> Result<AlarmRecord, String> {
    require_login(&state)?;
    let now = chrono::Utc::now().timestamp_millis();
    let updated = {
        let mut file = state.alarm_records.lock();
        let record = file
            .records
            .iter_mut()
            .find(|record| record.id == alarm_id)
            .ok_or("报警记录不存在")?;
        record.status = "resolved".into();
        record.resolved_at = Some(now);
        if let Some(note) = note {
            record.note = Some(note);
        }
        record.clone()
    };
    state.persist_alarm_records().map_err(|e| e.to_string())?;
    {
        let mut runtime = state.runtime_status.lock();
        if let Some(entry) = runtime.get_mut(&updated.source_id) {
            entry.alarm_status = updated.status.clone();
            entry.ts = now;
        }
    }
    let _ = app.emit(
        "ecoalert://alarm",
        serde_json::json!({
            "alarm_id": updated.id,
            "source_id": updated.source_id,
            "status": updated.status,
            "event": "alarm_resolved",
            "ts": now,
        }),
    );
    log_event(&app, "info", "报警已恢复");
    Ok(updated)
}

/* -------------------- 配置：算法 / ROI / 通知 / 安全 -------------------- */

#[tauri::command]
pub fn get_algorithm_config(
    state: State<Arc<AppState>>,
    source_id: Option<String>,
) -> Result<AlgorithmConfig, String> {
    require_login(&state)?;
    let cfg = state.algorithm_config.lock();
    if let Some(id) = source_id {
        Ok(cfg
            .sources
            .get(&id)
            .cloned()
            .unwrap_or_else(|| cfg.global.clone()))
    } else {
        Ok(cfg.global.clone())
    }
}

#[tauri::command]
pub fn list_algorithm_config_sources(state: State<Arc<AppState>>) -> Result<Vec<String>, String> {
    require_login(&state)?;
    let cfg = state.algorithm_config.lock();
    let mut ids: Vec<String> = cfg.sources.keys().cloned().collect();
    ids.sort();
    Ok(ids)
}

#[tauri::command]
pub fn get_effective_algorithm_config(
    state: State<Arc<AppState>>,
    source_id: String,
) -> Result<serde_json::Value, String> {
    require_login(&state)?;
    let source = {
        let sources = state.sources.lock();
        sources
            .iter()
            .find(|s| s.id == source_id)
            .cloned()
            .ok_or("视频源不存在")?
    };
    let cfg = state.algorithm_config.lock().clone();
    let effective = scheduler::effective_algorithm_config(&source, &cfg);
    Ok(serde_json::json!({
        "config": effective.config,
        "scope": effective.scope,
    }))
}

#[tauri::command]
pub fn update_algorithm_config(
    app: AppHandle,
    state: State<Arc<AppState>>,
    source_id: Option<String>,
    payload: AlgorithmConfig,
) -> Result<AlgorithmConfig, String> {
    require_login(&state)?;
    let mut saved = payload;
    {
        let mut cfg = state.algorithm_config.lock();
        if let Some(id) = source_id {
            saved.scope = "source".into();
            saved.scope_id = Some(id.clone());
            cfg.sources.insert(id, saved.clone());
        } else {
            saved.scope = "global".into();
            saved.scope_id = None;
            cfg.global = saved.clone();
        }
    }
    state
        .persist_algorithm_config()
        .map_err(|e| e.to_string())?;
    log_event(&app, "info", "算法配置已保存");
    Ok(saved)
}

#[tauri::command]
pub fn delete_algorithm_config(
    app: AppHandle,
    state: State<Arc<AppState>>,
    source_id: String,
) -> Result<serde_json::Value, String> {
    require_login(&state)?;
    {
        let mut cfg = state.algorithm_config.lock();
        cfg.sources.remove(&source_id);
    }
    state
        .persist_algorithm_config()
        .map_err(|e| e.to_string())?;
    log_event(&app, "info", "通道算法配置已恢复为全局默认设置");
    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub async fn test_vlm_config(
    state: State<'_, Arc<AppState>>,
    payload: AlgorithmConfig,
) -> Result<serde_json::Value, String> {
    require_login(&state)?;
    let result = vlm::test_connection(&payload)
        .await
        .map_err(|err| err.to_string())?;
    let cost = result
        .usage
        .as_ref()
        .map(|usage| calculate_vlm_cost(usage, &payload))
        .unwrap_or(0.0);
    Ok(serde_json::json!({
        "ok": true,
        "reply": result.reply,
        "usage": result.usage,
        "cost": cost,
        "requestUrl": result.request_url,
        "requestBody": result.request_body,
    }))
}

fn calculate_vlm_cost(usage: &vlm::VlmUsage, config: &AlgorithmConfig) -> f32 {
    let normal_input = usage
        .prompt_tokens
        .saturating_sub(usage.prompt_cached_tokens) as f32;
    let cached_input = usage.prompt_cached_tokens as f32;
    let normal_output = usage
        .completion_tokens
        .saturating_sub(usage.completion_cached_tokens) as f32;
    let cached_output = usage.completion_cached_tokens as f32;
    (normal_input * config.vlm_price_input
        + cached_input * config.vlm_price_input_cache
        + normal_output * config.vlm_price_output
        + cached_output * config.vlm_price_output_cache)
        / 1_000_000.0
}

#[tauri::command]
pub fn get_roi_config(
    state: State<Arc<AppState>>,
    source_id: Option<String>,
) -> Result<RoiConfig, String> {
    require_login(&state)?;
    let cfg = state.roi_config.lock();
    let Some(source_id) = source_id.filter(|id| id != GLOBAL_ROI_SOURCE_ID) else {
        return Ok(cfg.global.clone());
    };
    Ok(cfg.effective_for_source(&source_id))
}

#[tauri::command]
pub fn list_roi_config_sources(state: State<Arc<AppState>>) -> Result<Vec<String>, String> {
    require_login(&state)?;
    let cfg = state.roi_config.lock();
    let mut ids: Vec<String> = cfg.by_source.keys().cloned().collect();
    ids.sort();
    Ok(ids)
}

#[tauri::command]
pub fn update_roi_config(
    app: AppHandle,
    state: State<Arc<AppState>>,
    source_id: Option<String>,
    mut payload: RoiConfig,
) -> Result<RoiConfig, String> {
    require_login(&state)?;
    let is_global = source_id
        .as_deref()
        .map(|id| id == GLOBAL_ROI_SOURCE_ID)
        .unwrap_or(true);
    let saved_source_id = source_id.unwrap_or_else(|| GLOBAL_ROI_SOURCE_ID.into());
    payload.source_id = saved_source_id.clone();
    payload.updated_at = chrono::Utc::now().timestamp_millis();
    {
        let mut cfg = state.roi_config.lock();
        if is_global {
            cfg.global = payload.clone();
        } else {
            cfg.by_source.insert(saved_source_id, payload.clone());
        }
    }
    state.persist_roi_config().map_err(|e| e.to_string())?;
    log_event(
        &app,
        "info",
        if is_global {
            "全局默认设置已保存"
        } else {
            "通道 ROI 配置已保存"
        },
    );
    Ok(payload)
}

#[tauri::command]
pub fn delete_roi_config(
    app: AppHandle,
    state: State<Arc<AppState>>,
    source_id: String,
) -> Result<serde_json::Value, String> {
    require_login(&state)?;
    {
        let mut cfg = state.roi_config.lock();
        cfg.by_source.remove(&source_id);
    }
    state.persist_roi_config().map_err(|e| e.to_string())?;
    log_event(&app, "info", "通道 ROI 配置已恢复为全局默认设置");
    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub fn test_roi_config(
    state: State<Arc<AppState>>,
    source_id: String,
    payload: Option<RoiConfig>,
) -> Result<serde_json::Value, String> {
    require_login(&state)?;
    let source = state
        .sources
        .lock()
        .iter()
        .find(|source| source.id == source_id)
        .cloned()
        .ok_or("视频源不存在")?;
    let roi_config = if let Some(mut payload) = payload {
        payload.source_id = source_id.clone();
        payload
    } else {
        let cfg = state.roi_config.lock();
        cfg.effective_for_source(&source_id)
    };
    let frame =
        extract_gray_frame_from_url(&source.url, 160, 90, std::time::Duration::from_secs(5))
            .map_err(|err| format!("真实帧抽取失败: {err}"))?;
    let mut detector = Detector::new(PipelineConfig::default());
    let result = detector.analyze_scene(&frame, Some(&roi_config));
    Ok(serde_json::json!({
        "ok": true,
        "light": result.scene.light,
        "lightState": if result.scene.light { "on" } else { "off" },
        "person": result.scene.person,
        "brightness": result.light_brightness,
        "colorScore": result.scene.color_score,
        "motionScore": result.motion_score,
        "confidence": result.scene.confidence,
        "processMs": result.process_ms,
        "version": roi_config.version,
    }))
}

#[tauri::command]
pub fn list_notification_targets(
    state: State<Arc<AppState>>,
) -> Result<Vec<NotificationTarget>, String> {
    require_login(&state)?;
    Ok(state.notification_config.lock().targets.clone())
}

#[tauri::command]
pub fn create_notification_target(
    app: AppHandle,
    state: State<Arc<AppState>>,
    payload: NotificationTargetPayload,
) -> Result<NotificationTarget, String> {
    require_login(&state)?;
    if payload.name.trim().is_empty() || payload.url.trim().is_empty() {
        return Err("通知名称和 URL 不能为空".into());
    }
    let target = NotificationTarget::new(payload);
    {
        let mut cfg = state.notification_config.lock();
        cfg.targets.push(target.clone());
    }
    state
        .persist_notification_config()
        .map_err(|e| e.to_string())?;
    log_event(&app, "info", format!("新增通知目标: {}", target.name));
    Ok(target)
}

#[tauri::command]
pub fn update_notification_target(
    app: AppHandle,
    state: State<Arc<AppState>>,
    id: String,
    payload: NotificationTargetPayload,
) -> Result<NotificationTarget, String> {
    require_login(&state)?;
    let updated = {
        let mut cfg = state.notification_config.lock();
        let idx = cfg
            .targets
            .iter()
            .position(|x| x.id == id)
            .ok_or("通知目标不存在")?;
        let created_at = cfg.targets[idx].created_at;
        let mut item = NotificationTarget::new(payload);
        item.id = id;
        item.created_at = created_at;
        cfg.targets[idx] = item.clone();
        item
    };
    state
        .persist_notification_config()
        .map_err(|e| e.to_string())?;
    log_event(&app, "info", format!("更新通知目标: {}", updated.name));
    Ok(updated)
}

#[tauri::command]
pub fn delete_notification_target(
    app: AppHandle,
    state: State<Arc<AppState>>,
    id: String,
) -> Result<serde_json::Value, String> {
    require_login(&state)?;
    {
        let mut cfg = state.notification_config.lock();
        let idx = cfg
            .targets
            .iter()
            .position(|x| x.id == id)
            .ok_or("通知目标不存在")?;
        cfg.targets.remove(idx);
    }
    state
        .persist_notification_config()
        .map_err(|e| e.to_string())?;
    log_event(&app, "info", "删除通知目标");
    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub fn list_notification_history(
    state: State<Arc<AppState>>,
    source_id: Option<String>,
    event: Option<String>,
    ok: Option<bool>,
    limit: Option<usize>,
) -> Result<Vec<NotificationRecord>, String> {
    require_login(&state)?;
    let mut records: Vec<NotificationRecord> = state
        .notification_history
        .lock()
        .records
        .iter()
        .filter(|record| {
            source_id
                .as_deref()
                .map_or(true, |id| record.source_id.as_deref() == Some(id))
        })
        .filter(|record| event.as_deref().map_or(true, |ev| record.event == ev))
        .filter(|record| ok.map_or(true, |expected| record.ok == expected))
        .cloned()
        .collect();
    records.reverse();
    records.truncate(limit.unwrap_or(100));
    Ok(records)
}

#[tauri::command]
pub async fn test_notification_target(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: Option<String>,
    payload: Option<NotificationTargetPayload>,
) -> Result<NotificationRecord, String> {
    require_login(&state)?;
    let target = if let Some(id) = id {
        state
            .notification_config
            .lock()
            .targets
            .iter()
            .find(|target| target.id == id)
            .cloned()
            .ok_or("通知目标不存在")?
    } else {
        notifier::target_from_payload(payload.ok_or("缺少通知目标配置")?)
    };
    Ok(notifier::send_test(app, state.inner().clone(), target).await)
}

#[tauri::command]
pub async fn resend_notification(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    record_id: String,
) -> Result<NotificationRecord, String> {
    require_login(&state)?;
    let original = state
        .notification_history
        .lock()
        .records
        .iter()
        .find(|record| record.id == record_id)
        .cloned()
        .ok_or("通知历史不存在")?;
    let target = state
        .notification_config
        .lock()
        .targets
        .iter()
        .find(|target| target.id == original.target_id)
        .cloned()
        .ok_or("通知目标不存在")?;
    Ok(notifier::resend_record(app, state.inner().clone(), target, original).await)
}

#[tauri::command]
pub fn get_security_config(state: State<Arc<AppState>>) -> Result<SecurityConfig, String> {
    require_login(&state)?;
    Ok(state.security_config.lock().clone())
}

#[tauri::command]
pub fn update_security_config(
    app: AppHandle,
    state: State<Arc<AppState>>,
    payload: SecurityConfig,
) -> Result<SecurityConfig, String> {
    require_login(&state)?;
    {
        let mut cfg = state.security_config.lock();
        *cfg = payload.clone();
    }
    state.persist_security_config().map_err(|e| e.to_string())?;
    log_event(&app, "info", "安全配置已保存");
    Ok(payload)
}

/* -------------------- 其它 -------------------- */

#[tauri::command]
pub fn change_password(
    app: AppHandle,
    state: State<Arc<AppState>>,
    old_password: String,
    new_password: String,
) -> Result<serde_json::Value, String> {
    if new_password.len() < 6 {
        return Err("新密码至少 6 位".into());
    }
    let ok = state.auth.lock().verify(&old_password);
    if !ok {
        return Err("当前密码错误".into());
    }
    state.auth.lock().change_password(&new_password);
    state.persist_auth().map_err(|e| e.to_string())?;
    log_event(&app, "info", "登录密码已修改");
    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub fn get_data_dir(state: State<Arc<AppState>>) -> Result<String, String> {
    Ok(state.data_dir_str())
}

// ==================== OAuth / 凭证验证 ====================

use crate::pipeline::channel_auth;
use crate::pipeline::oauth_server::{self, OAuthSession};
use parking_lot::Mutex;
use std::collections::HashMap;

/// 活跃的 OAuth 会话
static OAUTH_SESSIONS: std::sync::LazyLock<Mutex<HashMap<String, OAuthSession>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// 启动 OAuth 绑定（目前支持飞书）
#[tauri::command]
pub async fn start_oauth_binding(
    _app: AppHandle,
    channel_type: String,
    app_id: String,
    _app_secret: String,
) -> Result<serde_json::Value, String> {
    if channel_type != "feishu" {
        return Err(format!("渠道 {channel_type} 暂不支持 OAuth 扫码"));
    }

    let session = OAuthSession::start()?;
    let port = session.port;
    let auth_url = session.feishu_auth_url(&app_id);
    let session_id = uuid::Uuid::new_v4().simple().to_string();

    // 保存 session 和相关凭证
    OAUTH_SESSIONS.lock().insert(session_id.clone(), session);

    // 同时在 state 里临时存储 app_id/app_secret 供后续换 token 用
    // 这里简单处理：用 session_id 作为 key 存到内存

    Ok(serde_json::json!({
        "sessionId": session_id,
        "port": port,
        "authUrl": auth_url,
        "qrData": auth_url,  // 前端用这个生成二维码
    }))
}

/// 检查 OAuth 状态（前端轮询）
#[tauri::command]
pub async fn check_oauth_status(
    _app: AppHandle,
    session_id: String,
    app_id: String,
    app_secret: String,
) -> Result<serde_json::Value, String> {
    let code = {
        let sessions = OAUTH_SESSIONS.lock();
        let session = sessions
            .get(&session_id)
            .ok_or("OAuth 会话不存在或已过期")?;
        if session.is_done() {
            session.auth_code.lock().clone()
        } else {
            None
        }
    };

    if let Some(code) = code {
        OAUTH_SESSIONS.lock().remove(&session_id);

        let (token, _expires) =
            oauth_server::exchange_feishu_token(&app_id, &app_secret, &code).await?;

        // 获取群列表
        let chats = oauth_server::list_feishu_chats(&token)
            .await
            .unwrap_or_default();

        return Ok(serde_json::json!({
            "status": "success",
            "accessToken": token,
            "chats": chats,
        }));
    }

    Ok(serde_json::json!({ "status": "pending" }))
}

/// 验证凭证是否有效
#[tauri::command]
pub async fn verify_channel_credentials(
    _app: AppHandle,
    channel_type: String,
    app_id: String,
    app_secret: String,
) -> Result<serde_json::Value, String> {
    channel_auth::verify_credentials(&channel_type, &app_id, &app_secret).await?;
    Ok(serde_json::json!({ "ok": true, "message": "凭证验证通过" }))
}

/// 写入 WebView 侧诊断日志。
/// 注意：Tauri 2 当前这里不直接打开 DevTools，只辅助排查视频流加载问题。
#[tauri::command]
pub fn open_devtools(app: AppHandle) -> Result<(), String> {
    if let Some(wv) = app.get_webview_window("main") {
        let _ = wv.eval("console.log('=== EcoAlert DevTools Probe ===')");
        let _ = wv.eval("console.log('CSP:', document.querySelector('meta[http-equiv=Content-Security-Policy]')?.content || '(none in meta)')");
        let _ = wv.eval(
            "fetch('http://127.0.0.1:8080/cam-1/index.m3u8').then(r => console.log('probe m3u8 status:', r.status)).catch(e => console.error('probe m3u8 FAIL:', e))",
        );
        Ok(())
    } else {
        Err("找不到主窗口".into())
    }
}

/// 测试一个 URL 能否从 Rust 后端访问（用于区分推流器不可达和 WebView 播放问题）。
#[tauri::command]
pub async fn probe_url(url: String) -> Result<serde_json::Value, String> {
    use std::time::Duration;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let ok = status.is_success();
    let body_len = resp.content_length().unwrap_or(0);
    Ok(serde_json::json!({
        "ok": ok,
        "status": status.as_u16(),
        "content_length": body_len,
    }))
}
