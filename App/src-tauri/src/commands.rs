// 暴露给前端的 Tauri commands
use crate::pipeline::{
    decoder::{extract_gray_frame_from_url, resolve_media_tool},
    detector::Detector,
    notifier, scheduler, vlm, PipelineConfig,
};
use crate::state::{log_event, AppState};
use crate::store::{
    backfill_groups, records_by_source, seed_local_hls_sources, AlarmRecord, AlarmRecordFile,
    AlgorithmConfig, AlgorithmConfigFile, ChannelRuntimeStatus, DataFile, DetectionHistoryFile,
    DetectionSampleRecord, HistoryFile, NotificationConfigFile, NotificationHistoryFile,
    NotificationRecord, NotificationTarget, NotificationTargetPayload, RoiConfig, RoiConfigFile,
    SceneState, SecurityConfig, SourceGroup, StateRecord, VideoSource, GLOBAL_ROI_SOURCE_ID,
    TEST_GROUP_IDS, TEST_SOURCE_IDS,
};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
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

/* -------------------- 调试菜单：测试视频源开关 -------------------- */

const TEST_VIDEO_GROUP_ID: &str = "grp-test-videos";
const VIDEO_FILE_EXTS: &[&str] = &["mp4", "m4v", "mov", "mkv", "avi", "webm"];

#[derive(serde::Serialize)]
pub struct ImportTestSourcesResult {
    pub sources: Vec<VideoSource>,
    pub imported: usize,
    pub skipped: usize,
}

fn is_video_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            VIDEO_FILE_EXTS
                .iter()
                .any(|item| item.eq_ignore_ascii_case(ext))
        })
        .unwrap_or(false)
}

fn collect_video_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_video_files(&path, out)?;
        } else if path.is_file() && is_video_file(&path) {
            out.push(path);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn import_test_sources_from_folder(
    app: AppHandle,
    state: State<Arc<AppState>>,
    folder_path: String,
) -> Result<ImportTestSourcesResult, String> {
    require_login(&state)?;
    let folder = PathBuf::from(folder_path.trim());
    if !folder.is_dir() {
        return Err("请选择有效的视频文件夹".into());
    }

    let mut files = Vec::new();
    collect_video_files(&folder, &mut files).map_err(|e| format!("扫描视频文件失败: {e}"))?;
    files.sort();
    if files.is_empty() {
        return Err("所选文件夹中没有可导入的视频文件".into());
    }

    {
        let mut groups = state.groups.lock();
        if !groups.iter().any(|g| g.id == TEST_VIDEO_GROUP_ID) {
            let next_order = groups.iter().map(|g| g.order).max().unwrap_or(0) + 1;
            groups.push(SourceGroup {
                id: TEST_VIDEO_GROUP_ID.into(),
                name: "测试视频".into(),
                order: next_order,
                collapsed: false,
                domain_detection_enabled: false,
                created_at: chrono::Utc::now().timestamp_millis(),
            });
        }
    }

    let remove_ids: std::collections::HashSet<&str> = TEST_SOURCE_IDS.iter().copied().collect();
    let mut imported = 0usize;
    let mut skipped = 0usize;
    {
        let mut sources = state.sources.lock();
        sources.retain(|s| !remove_ids.contains(s.id.as_str()));
        let mut existing_urls: std::collections::HashSet<String> =
            sources.iter().map(|s| s.url.clone()).collect();
        let base_order = sources
            .iter()
            .filter(|s| s.group_id.as_deref() == Some(TEST_VIDEO_GROUP_ID))
            .map(|s| s.order)
            .max()
            .unwrap_or(-1)
            + 1;

        for file in files {
            let url = file.to_string_lossy().into_owned();
            if existing_urls.contains(&url) {
                skipped += 1;
                continue;
            }
            let name = file
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("测试视频")
                .chars()
                .take(64)
                .collect::<String>();
            let location = file
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(String::new)
                .chars()
                .take(128)
                .collect::<String>();
            sources.push(VideoSource::new(
                name,
                url.clone().chars().take(512).collect(),
                "mp4".into(),
                location,
                true,
                Some(TEST_VIDEO_GROUP_ID.into()),
                base_order + imported as i32,
            ));
            existing_urls.insert(url);
            imported += 1;
        }
    }

    state
        .groups
        .lock()
        .retain(|g| !TEST_GROUP_IDS.contains(&g.id.as_str()));
    state.persist_sources().map_err(|e| e.to_string())?;
    log_event(
        &app,
        "info",
        format!("已从文件夹导入测试视频源: {imported} 个，跳过 {skipped} 个"),
    );
    Ok(ImportTestSourcesResult {
        sources: state.sources.lock().clone(),
        imported,
        skipped,
    })
}

/// 开启时创建 8 个预设 HLS 测试源，关闭时只删除这 8 个（用户自加的源保留）。
#[tauri::command]
pub fn set_test_sources_enabled(
    app: AppHandle,
    state: State<Arc<AppState>>,
    enabled: bool,
) -> Result<Vec<VideoSource>, String> {
    require_login(&state)?;
    if enabled {
        // 用临时 DataFile 收集 seed 结果，再合并到 state（避免覆盖用户已有数据）。
        let mut tmp = crate::store::DataFile::default();
        seed_local_hls_sources(&mut tmp);
        let existing_ids: std::collections::HashSet<String> =
            state.sources.lock().iter().map(|s| s.id.clone()).collect();
        let mut sources = state.sources.lock();
        for s in tmp.sources {
            if !existing_ids.contains(&s.id) {
                sources.push(s);
            }
        }
        drop(sources);
        let existing_group_ids: std::collections::HashSet<String> =
            state.groups.lock().iter().map(|g| g.id.clone()).collect();
        let mut groups = state.groups.lock();
        for g in tmp.groups {
            if !existing_group_ids.contains(&g.id) {
                groups.push(g);
            }
        }
        drop(groups);
        log_event(&app, "info", "测试视频源已创建");
    } else {
        let remove_ids: std::collections::HashSet<&str> = TEST_SOURCE_IDS.iter().copied().collect();
        state.sources.lock().retain(|s| {
            !remove_ids.contains(s.id.as_str())
                && s.group_id.as_deref() != Some(TEST_VIDEO_GROUP_ID)
        });
        // 仅当测试分组下已无任何源时才删除分组
        let remaining_group_ids: std::collections::HashSet<Option<String>> = state
            .sources
            .lock()
            .iter()
            .map(|s| s.group_id.clone())
            .collect();
        state.groups.lock().retain(|g| {
            (g.id != TEST_VIDEO_GROUP_ID && !TEST_GROUP_IDS.contains(&g.id.as_str()))
                || remaining_group_ids.contains(&Some(g.id.clone()))
        });
        log_event(&app, "info", "测试视频源已移除");
    }
    state.persist_sources().map_err(|e| e.to_string())?;
    Ok(state.sources.lock().clone())
}

/* -------------------- 分组 -------------------- */

#[derive(serde::Deserialize)]
pub struct GroupPayload {
    pub name: String,
    #[serde(default)]
    pub order: i32,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub domain_detection_enabled: bool,
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
    grp.domain_detection_enabled = payload.domain_detection_enabled;
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
        domain_detection_enabled: payload.domain_detection_enabled,
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
    let cost = if payload.vlm_cost_enabled {
        result
            .usage
            .as_ref()
            .map(|usage| calculate_vlm_cost(usage, &payload))
    } else {
        None
    };
    Ok(serde_json::json!({
        "ok": true,
        "reply": result.reply,
        "usage": result.usage,
        "costEnabled": payload.vlm_cost_enabled,
        "cost": cost,
        "requestUrl": result.request_url,
        "requestBody": result.request_body,
    }))
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TestVlmVisionPayload {
    #[serde(flatten)]
    pub config: AlgorithmConfig,
    #[serde(alias = "sourceId")]
    pub source_id: String,
}

/// 用指定视频源抽一帧，走真实 VLM 图片识别流程，返回模型原始回复和请求体。
#[tauri::command]
pub async fn test_vlm_vision(
    state: State<'_, Arc<AppState>>,
    payload: TestVlmVisionPayload,
) -> Result<serde_json::Value, String> {
    require_login(&state)?;
    let sources = state.sources.lock().clone();
    let source = sources
        .iter()
        .find(|s| s.id == payload.source_id)
        .ok_or_else(|| format!("未找到视频源 {}", payload.source_id))?;
    let url = source.url.clone();
    let frame = tokio::task::spawn_blocking(move || {
        crate::pipeline::decoder::extract_original_frame_from_url_at(
            &url,
            std::time::Duration::from_secs(10),
            None,
        )
    })
    .await
    .map_err(|e| format!("抽帧任务异常: {e}"))?
    .map_err(|e| format!("抽帧失败: {e}"))?;
    let result = vlm::test_vision(&payload.config, &frame)
        .await
        .map_err(|err| err.to_string())?;
    let cost = if payload.config.vlm_cost_enabled {
        result
            .usage
            .as_ref()
            .map(|usage| calculate_vlm_cost(usage, &payload.config))
    } else {
        None
    };
    Ok(serde_json::json!({
        "ok": true,
        "reply": result.reply,
        "usage": result.usage,
        "costEnabled": payload.config.vlm_cost_enabled,
        "cost": cost,
        "requestUrl": result.request_url,
        "requestBody": result.request_body,
    }))
}

/// 测试 YOLO WebSocket 服务器连通性（连接 /ws 发一帧最小占位图）
#[tauri::command]
pub async fn test_yolo_connection(
    state: State<'_, Arc<AppState>>,
    api_base: String,
) -> Result<serde_json::Value, String> {
    require_login(&state)?;

    let url = crate::pipeline::yolo_detector::YoloClient::new(api_base).ws_url()?;

    let connect = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio_tungstenite::connect_async(&url),
    )
    .await
    .map_err(|_| "连接超时（>3s）".to_string())?
    .map_err(|e| format!("连接失败: {e}"))?;
    let (mut ws, _resp) = connect;

    // 发一张 8x8 黑色 JPEG（最小有效帧），等响应
    let frame = crate::pipeline::decoder::DecodedFrame {
        width: 8,
        height: 8,
        pts_ms: chrono::Utc::now().timestamp_millis(),
        data: vec![0u8; 64],
        rgb: vec![0u8; 64 * 3],
    };
    let jpeg_bytes = crate::pipeline::yolo_detector::encode_frame_as_jpeg(&frame)
        .map_err(|e| format!("测试帧编码失败: {e}"))?;
    ws.send(tokio_tungstenite::tungstenite::Message::Binary(
        jpeg_bytes.into(),
    ))
    .await
    .map_err(|e| format!("发送测试帧失败: {e}"))?;

    // 循环接收，跳过 Ping/Pong/Frame 等控制帧（tungstenite 0.29 暴露了这些变体）
    let text = loop {
        let recv = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
            .await
            .map_err(|_| "等待响应超时（>5s）".to_string())?;
        let msg = match recv {
            Some(Ok(m)) => m,
            Some(Err(e)) => return Err(format!("接收失败: {e}")),
            None => return Err("服务器关闭连接".into()),
        };
        match msg {
            tokio_tungstenite::tungstenite::Message::Text(t) => break t,
            tokio_tungstenite::tungstenite::Message::Close(_) => {
                return Err("服务器主动关闭".into());
            }
            tokio_tungstenite::tungstenite::Message::Ping(_)
            | tokio_tungstenite::tungstenite::Message::Pong(_)
            | tokio_tungstenite::tungstenite::Message::Frame(_) => continue,
            other => return Err(format!("收到非预期消息类型: {other:?}")),
        }
    };

    // 关连接
    let _ = ws
        .send(tokio_tungstenite::tungstenite::Message::Close(None))
        .await;
    let body: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        let snippet = text.chars().take(200).collect::<String>();
        format!("解析响应失败: {e} (text={snippet})")
    })?;
    if let Some(err) = body.get("error").and_then(|v| v.as_str()) {
        return Err(format!("服务端返回错误: {err}"));
    }
    let count = body
        .get("count")
        .and_then(|v| v.as_i64())
        .unwrap_or_default();
    let process_ms = body
        .get("process_ms")
        .and_then(|v| v.as_f64())
        .unwrap_or_default();
    Ok(serde_json::json!({
        "ok": true,
        "url": crate::pipeline::yolo_detector::redact_url(&url),
        "count": count,
        "processMs": process_ms,
        "raw": body,
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
    validate_notification_payload(&payload)?;
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
    validate_notification_payload(&payload)?;
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

fn validate_notification_payload(payload: &NotificationTargetPayload) -> Result<(), String> {
    if payload.name.trim().is_empty() {
        return Err("通知名称不能为空".into());
    }
    let is_api_mode = payload.channel_type != "webhook" && !payload.app_id.trim().is_empty();
    if is_api_mode {
        if payload.app_secret.trim().is_empty() {
            return Err("API 模式下 App Secret 不能为空".into());
        }
        if payload.channel_type != "qqbot" && payload.chat_id.trim().is_empty() {
            return Err("API 模式下接收目标不能为空".into());
        }
        if payload.channel_type == "wechat_work" && payload.agent_id.trim().is_empty() {
            return Err("企业微信 API 模式下 Agent ID 不能为空".into());
        }
    } else if payload.url.trim().is_empty() {
        return Err("Webhook URL 不能为空".into());
    }
    Ok(())
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

#[tauri::command]
pub fn reset_all_app_data(
    app: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    require_login(&state)?;
    {
        let mut data = DataFile::default();
        backfill_groups(&mut data);
        *state.sources.lock() = data.sources.clone();
        *state.groups.lock() = data.groups.clone();
        *state.data.lock() = data;
    }
    {
        *state.history.lock() = HistoryFile::default();
        *state.detection_history.lock() = DetectionHistoryFile::default();
        *state.algorithm_config.lock() = AlgorithmConfigFile::default();
        *state.roi_config.lock() = RoiConfigFile::default();
        *state.notification_config.lock() = NotificationConfigFile::default();
        *state.security_config.lock() = SecurityConfig::default();
        *state.alarm_records.lock() = AlarmRecordFile::default();
        *state.notification_history.lock() = NotificationHistoryFile::default();
        state.current_state.lock().clear();
        state.runtime_status.lock().clear();
    }

    crate::store::save(&state.data_file, &*state.data.lock()).map_err(|e| e.to_string())?;
    crate::store::save_history(&state.history_file, &*state.history.lock())
        .map_err(|e| e.to_string())?;
    crate::store::save_json(
        &state.detection_history_file,
        &*state.detection_history.lock(),
    )
    .map_err(|e| e.to_string())?;
    state
        .persist_algorithm_config()
        .map_err(|e| e.to_string())?;
    state.persist_roi_config().map_err(|e| e.to_string())?;
    state
        .persist_notification_config()
        .map_err(|e| e.to_string())?;
    state.persist_security_config().map_err(|e| e.to_string())?;
    state.persist_alarm_records().map_err(|e| e.to_string())?;
    crate::store::save_json(
        &state.notification_history_file,
        &*state.notification_history.lock(),
    )
    .map_err(|e| e.to_string())?;

    let sources = state.sources.lock().clone();
    app.emit("ecoalert://sources", sources.clone())
        .map_err(|e| e.to_string())?;
    log_event(&app, "warn", "已初始化全部业务配置和运行状态");
    Ok(serde_json::json!({ "ok": true, "sources": sources }))
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

#[tauri::command]
pub fn check_ffmpeg_status() -> Result<serde_json::Value, String> {
    fn check_tool(name: &str) -> serde_json::Value {
        let path = resolve_media_tool(name);
        let mut command = Command::new(&path);
        command
            .arg("-version")
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                let message = if err.kind() == std::io::ErrorKind::NotFound {
                    format!(
                        "未找到 {name}，请将 {name}.exe 放到程序目录，或把 ffmpeg 安装目录加入 PATH"
                    )
                } else {
                    format!("{name} 启动失败: {err}")
                };
                return serde_json::json!({
                    "ok": false,
                    "path": path.to_string_lossy(),
                    "version": null,
                    "error": message,
                });
            }
        };
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if started.elapsed() > Duration::from_secs(3) {
                        let _ = child.kill();
                        let _ = child.wait();
                        return serde_json::json!({
                            "ok": false,
                            "path": path.to_string_lossy(),
                            "version": null,
                            "error": format!("{name} 检测超时"),
                        });
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(err) => {
                    return serde_json::json!({
                        "ok": false,
                        "path": path.to_string_lossy(),
                        "version": null,
                        "error": format!("{name} 检测失败: {err}"),
                    });
                }
            }
        };
        let mut output = String::new();
        if let Some(mut stdout) = child.stdout.take() {
            let _ = std::io::Read::read_to_string(&mut stdout, &mut output);
        }
        let first_line = output.lines().next().unwrap_or("").trim().to_string();
        serde_json::json!({
            "ok": status.success(),
            "path": path.to_string_lossy(),
            "version": if first_line.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(first_line) },
            "error": if status.success() { serde_json::Value::Null } else { serde_json::Value::String(format!("{name} 退出码: {status}")) },
        })
    }

    let ffmpeg = check_tool("ffmpeg");
    let ffprobe = check_tool("ffprobe");
    let ok = ffmpeg["ok"].as_bool().unwrap_or(false) && ffprobe["ok"].as_bool().unwrap_or(false);
    Ok(serde_json::json!({
        "ok": ok,
        "ffmpeg": ffmpeg,
        "ffprobe": ffprobe,
    }))
}

// ==================== OAuth / 凭证验证 ====================

use crate::pipeline::channel_auth;
use crate::pipeline::oauth_server::{self, OAuthSession};
use futures_util::{SinkExt, StreamExt};
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
