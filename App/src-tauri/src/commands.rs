// 暴露给前端的 Tauri commands
use crate::state::{log_event, AppState};
use crate::store::{records_by_source, SceneState, SourceGroup, StateRecord, VideoSource};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

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
    log_event(&app, "info", format!("新增视频源: {} ({})", item.name, item.url));
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
    let idx = sources.iter().position(|s| s.id == id).ok_or("视频源不存在")?;
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
        let idx = sources.iter().position(|s| s.id == id).ok_or("视频源不存在")?;
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
    log_event(&app, "debug", format!("重排 {} 项", /*items count*/ 0));
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
    let scene = SceneState { person, light, frame_seq: 0, confidence: 1.0 };
    let rec = StateRecord::from_change(&source_id, &scene);
    state.record_state_change(rec).map_err(|e| e.to_string())?;
    let _ = app.emit(
        "ecoalert://scene_state",
        serde_json::json!({
            "source_id": source_id,
            "person": person,
            "light": light,
            "ts": chrono::Utc::now().timestamp_millis(),
        }),
    );
    Ok(serde_json::json!({ "ok": true }))
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
    if !ok { return Err("当前密码错误".into()); }
    state.auth.lock().change_password(&new_password);
    state.persist_auth().map_err(|e| e.to_string())?;
    log_event(&app, "info", "登录密码已修改");
    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub fn get_data_dir(state: State<Arc<AppState>>) -> Result<String, String> {
    Ok(state.data_dir_str())
}
