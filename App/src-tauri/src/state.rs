// 应用状态：登录态、视频源、分组、状态推送、算法接入点
use crate::auth::AuthConfig;
use crate::store::{
    backfill_groups, load, load_history, save, save_history, DataFile, HistoryFile, SceneState,
    SourceGroup, StateRecord, VideoSource,
};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Serialize)]
pub struct ChannelStatus {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub bitrate: u32,
    pub fps: u32,
    pub viewers: u32,
    pub location: String,
    pub ts: i64,
}

pub struct AppState {
    pub data_dir: PathBuf,
    pub data_file: PathBuf,
    pub auth_file: PathBuf,
    pub history_file: PathBuf,
    pub logged_in: Mutex<bool>,
    pub sources: Mutex<Vec<VideoSource>>,
    pub groups: Mutex<Vec<SourceGroup>>,
    pub auth: Mutex<AuthConfig>,
    pub data: Mutex<DataFile>,
    pub history: Mutex<HistoryFile>,
    /// 每个源当前最近一次 SceneState（用于去重 / 比较）
    pub current_state: Mutex<HashMap<String, SceneState>>,
}

impl AppState {
    pub fn new(app: &AppHandle) -> anyhow::Result<Arc<Self>> {
        let data_dir = app
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| std::env::temp_dir().join("ecoalert"));
        std::fs::create_dir_all(&data_dir)?;
        let data_file = data_dir.join("sources.json");
        let auth_file = data_dir.join("auth.json");
        let history_file = data_dir.join("state_history.json");

        // 加载数据
        let mut data = load(&data_file);
        // 向前兼容：补全 group_id
        backfill_groups(&mut data);
        // 首次运行把补全后的结果落盘
        let _ = save(&data_file, &data);

        let auth = if auth_file.exists() {
            match std::fs::read_to_string(&auth_file) {
                Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
                Err(_) => AuthConfig::default(),
            }
        } else {
            let a = AuthConfig::default();
            let _ = std::fs::write(&auth_file, serde_json::to_string_pretty(&a).unwrap());
            a
        };

        let history = load_history(&history_file);
        let sources = data.sources.clone();
        let groups = data.groups.clone();

        Ok(Arc::new(Self {
            data_dir,
            data_file,
            auth_file,
            history_file,
            logged_in: Mutex::new(false),
            sources: Mutex::new(sources),
            groups: Mutex::new(groups),
            auth: Mutex::new(auth),
            data: Mutex::new(data),
            history: Mutex::new(history),
            current_state: Mutex::new(HashMap::new()),
        }))
    }

    pub fn data_dir_str(&self) -> String {
        self.data_dir.to_string_lossy().into_owned()
    }

    /// 同步内存里的 sources / groups，写回磁盘
    pub fn persist_sources(&self) -> anyhow::Result<()> {
        let mut data = self.data.lock();
        data.sources = self.sources.lock().clone();
        data.groups = self.groups.lock().clone();
        save(&self.data_file, &data)
    }

    pub fn persist_auth(&self) -> anyhow::Result<()> {
        let a = self.auth.lock().clone();
        std::fs::write(&self.auth_file, serde_json::to_string_pretty(&a)?)?;
        Ok(())
    }

    /// 历史记录落盘（每个源最多保留 200 条）
    pub fn record_state_change(&self, rec: StateRecord) -> anyhow::Result<()> {
        let mut h = self.history.lock();
        h.records.push(rec);
        // 简单的总量截断：保留最近 5000 条
        if h.records.len() > 5000 {
            let drop_n = h.records.len() - 5000;
            h.records.drain(0..drop_n);
        }
        save_history(&self.history_file, &h)
    }
}

/// 启动后台状态推送任务（码率/FPS/在线）
pub fn spawn_status_ticker(app: AppHandle) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3));
        loop {
            interval.tick().await;
            let state = app.state::<Arc<AppState>>();
            let sources = state.sources.lock().clone();
            let now = chrono::Utc::now().timestamp_millis();
            let stats: Vec<ChannelStatus> = sources
                .iter()
                .map(|s| {
                    let seed = (s.id.bytes().next().unwrap_or(0) as i64) + (now / 3000);
                    let online = s.enabled;
                    ChannelStatus {
                        id: s.id.clone(),
                        name: s.name.clone(),
                        online,
                        bitrate: if online {
                            ((seed.wrapping_mul(9301).wrapping_add(49297) % 23328).unsigned_abs()) + 800
                        } else {
                            0
                        },
                        fps: if online { 20 + ((seed.wrapping_mul(7) as u32) % 11) } else { 0 },
                        viewers: if online { ((seed.wrapping_mul(13) as u32) % 8) } else { 0 },
                        location: s.location.clone(),
                        ts: now,
                    }
                })
                .collect();
            let _ = app.emit("ecoalert://status", &stats);
        }
    });
}

/// 启动算法模拟推送任务（生产中替换为 pipeline::Pipeline 真实输出）
///
/// 算法接口契约（请勿修改）：
///   输入：每路视频的连续帧（pipeline 内部完成解码）
///   输出：SceneState { person: bool, light: bool, frame_seq, confidence }
///   推送：通过 Tauri event "ecoalert://scene_state" 发给前端
///   落库：状态变化时调用 record_state_change
pub fn spawn_scene_state_ticker(app: AppHandle) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(4));
        // 简单伪随机：每个源一个"状态种子"，随时间漂移
        let mut seeds: HashMap<String, (u64, bool, bool)> = HashMap::new();
        loop {
            interval.tick().await;
            let state = app.state::<Arc<AppState>>();
            let sources = state.sources.lock().clone();
            let now = chrono::Utc::now().timestamp_millis();
            for s in &sources {
                if !s.enabled { continue; }
                let id_bytes = s.id.bytes().next().unwrap_or(0) as u64;
                let (counter, prev_p, prev_l) = seeds
                    .entry(s.id.clone())
                    .or_insert((0, false, false));
                // 噪声：基于时间窗和源 id 的伪随机
                let t = now / 4000;
                let n = id_bytes.wrapping_mul(t);
                let person = (n % 5) < 2;          // ~40% 概率有人
                let light = (n.wrapping_mul(3) % 7) < 3; // ~43% 概率开灯
                *counter += 1;
                let new_state = SceneState {
                    person,
                    light,
                    frame_seq: *counter,
                    confidence: 0.75 + ((n % 50) as f32) / 200.0,
                };
                // 只在变化时落库 + 推送
                if person != *prev_p || light != *prev_l {
                    let rec = StateRecord::from_change(&s.id, &new_state);
                    if let Err(e) = state.record_state_change(rec) {
                        log::warn!("历史落库失败: {e}");
                    }
                    let _ = app.emit(
                        "ecoalert://scene_state",
                        serde_json::json!({
                            "source_id": s.id,
                            "person": person,
                            "light": light,
                            "ts": now,
                        }),
                    );
                    let alarm = !person && light;
                    log_event(
                        &app,
                        if alarm { "warn" } else { "info" },
                        format!(
                            "[{}] 状态变化: 人数={} 灯={}{}",
                            s.name,
                            if person { "●" } else { "○" },
                            if light { "●" } else { "○" },
                            if alarm { "  ⚠️ 报警" } else { "" }
                        ),
                    );
                    *prev_p = person;
                    *prev_l = light;
                } else {
                    // 即便没变化也定期推一次，让前端心跳（每 ~12s 一次）
                    if *counter % 3 == 0 {
                        let _ = app.emit(
                            "ecoalert://scene_state",
                            serde_json::json!({
                                "source_id": s.id,
                                "person": person,
                                "light": light,
                                "ts": now,
                            }),
                        );
                    }
                }
            }
        }
    });
}

/// 日志事件：emit + 写 stdout
pub fn log_event(app: &AppHandle, level: &str, text: impl Into<String>) {
    let payload = serde_json::json!({
        "type": "event",
        "level": level,
        "text": text.into(),
        "ts": chrono::Utc::now().timestamp_millis(),
    });
    let _ = app.emit("ecoalert://event", payload.clone());
    let text_str = payload["text"].as_str().unwrap_or("");
    match level {
        "error" => log::error!("{}", text_str),
        "warn" => log::warn!("{}", text_str),
        _ => log::info!("{}", text_str),
    }
}
