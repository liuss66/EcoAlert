// 应用状态：登录态、视频源、分组、状态推送、算法接入点
use crate::auth::AuthConfig;
use crate::pipeline::{notifier, scheduler};
use crate::store::{
    backfill_groups, load, load_history, load_json, save, save_history, save_json, AlarmRecord,
    AlarmRecordFile, AlgorithmConfigFile, ChannelRuntimeStatus, DataFile, HistoryFile,
    NotificationConfigFile, NotificationHistoryFile, NotificationRecord, RoiConfigFile, SceneState,
    SecurityConfig, SourceGroup, StateRecord, VideoSource,
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
    pub algorithm_config_file: PathBuf,
    pub roi_config_file: PathBuf,
    pub notification_config_file: PathBuf,
    pub security_config_file: PathBuf,
    pub alarm_records_file: PathBuf,
    pub notification_history_file: PathBuf,
    pub logged_in: Mutex<bool>,
    pub sources: Mutex<Vec<VideoSource>>,
    pub groups: Mutex<Vec<SourceGroup>>,
    pub auth: Mutex<AuthConfig>,
    pub data: Mutex<DataFile>,
    pub history: Mutex<HistoryFile>,
    pub algorithm_config: Mutex<AlgorithmConfigFile>,
    pub roi_config: Mutex<RoiConfigFile>,
    pub notification_config: Mutex<NotificationConfigFile>,
    pub security_config: Mutex<SecurityConfig>,
    pub alarm_records: Mutex<AlarmRecordFile>,
    pub notification_history: Mutex<NotificationHistoryFile>,
    /// 每个源当前最近一次 SceneState（用于去重 / 比较）
    pub current_state: Mutex<HashMap<String, SceneState>>,
    pub runtime_status: Mutex<HashMap<String, ChannelRuntimeStatus>>,
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
        let algorithm_config_file = data_dir.join("algorithm_config.json");
        let roi_config_file = data_dir.join("roi_config.json");
        let notification_config_file = data_dir.join("notification_config.json");
        let security_config_file = data_dir.join("security_config.json");
        let alarm_records_file = data_dir.join("alarm_records.json");
        let notification_history_file = data_dir.join("notification_history.json");

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
        let algorithm_config: AlgorithmConfigFile = load_json(&algorithm_config_file);
        let roi_config: RoiConfigFile = load_json(&roi_config_file);
        let notification_config: NotificationConfigFile = load_json(&notification_config_file);
        let security_config: SecurityConfig = load_json(&security_config_file);
        let alarm_records: AlarmRecordFile = load_json(&alarm_records_file);
        let notification_history: NotificationHistoryFile = load_json(&notification_history_file);
        let _ = save_json(&algorithm_config_file, &algorithm_config);
        let _ = save_json(&roi_config_file, &roi_config);
        let _ = save_json(&notification_config_file, &notification_config);
        let _ = save_json(&security_config_file, &security_config);
        let _ = save_json(&alarm_records_file, &alarm_records);
        let _ = save_json(&notification_history_file, &notification_history);
        let sources = data.sources.clone();
        let groups = data.groups.clone();

        Ok(Arc::new(Self {
            data_dir,
            data_file,
            auth_file,
            history_file,
            algorithm_config_file,
            roi_config_file,
            notification_config_file,
            security_config_file,
            alarm_records_file,
            notification_history_file,
            logged_in: Mutex::new(false),
            sources: Mutex::new(sources),
            groups: Mutex::new(groups),
            auth: Mutex::new(auth),
            data: Mutex::new(data),
            history: Mutex::new(history),
            algorithm_config: Mutex::new(algorithm_config),
            roi_config: Mutex::new(roi_config),
            notification_config: Mutex::new(notification_config),
            security_config: Mutex::new(security_config),
            alarm_records: Mutex::new(alarm_records),
            notification_history: Mutex::new(notification_history),
            current_state: Mutex::new(HashMap::new()),
            runtime_status: Mutex::new(HashMap::new()),
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

    pub fn persist_algorithm_config(&self) -> anyhow::Result<()> {
        save_json(&self.algorithm_config_file, &*self.algorithm_config.lock())
    }

    pub fn persist_roi_config(&self) -> anyhow::Result<()> {
        save_json(&self.roi_config_file, &*self.roi_config.lock())
    }

    pub fn persist_notification_config(&self) -> anyhow::Result<()> {
        save_json(
            &self.notification_config_file,
            &*self.notification_config.lock(),
        )
    }

    pub fn persist_security_config(&self) -> anyhow::Result<()> {
        save_json(&self.security_config_file, &*self.security_config.lock())
    }

    pub fn persist_alarm_records(&self) -> anyhow::Result<()> {
        save_json(&self.alarm_records_file, &*self.alarm_records.lock())
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

    pub fn runtime_status_snapshot(&self) -> Vec<ChannelRuntimeStatus> {
        let now = chrono::Utc::now().timestamp_millis();
        let sources = self.sources.lock().clone();
        let mut runtime = self.runtime_status.lock();
        for source in &sources {
            runtime.entry(source.id.clone()).or_insert_with(|| {
                ChannelRuntimeStatus::new(source.id.clone(), source.enabled, now)
            });
        }
        runtime.retain(|source_id, _| sources.iter().any(|s| &s.id == source_id));
        sources
            .iter()
            .filter_map(|source| runtime.get(&source.id).cloned())
            .collect()
    }

    pub fn apply_alarm_state(
        &self,
        source_id: &str,
        alarm: bool,
        state_record_id: Option<String>,
        ts: i64,
    ) -> anyhow::Result<Option<AlarmRecord>> {
        let mut file = self.alarm_records.lock();
        let active_idx = file.records.iter().position(|record| {
            record.source_id == source_id
                && matches!(
                    record.status.as_str(),
                    "suspected" | "alarm_active" | "acknowledged"
                )
        });

        let changed = if alarm {
            if let Some(idx) = active_idx {
                let record = &mut file.records[idx];
                record.last_state_id = state_record_id;
                if record.status == "suspected" {
                    record.status = "alarm_active".into();
                    record.triggered_at = Some(ts);
                }
                Some(record.clone())
            } else {
                let record = AlarmRecord::new_active(source_id.to_string(), ts, state_record_id);
                file.records.push(record.clone());
                Some(record)
            }
        } else if let Some(idx) = active_idx {
            let record = &mut file.records[idx];
            record.status = "resolved".into();
            record.resolved_at = Some(ts);
            record.last_state_id = state_record_id;
            Some(record.clone())
        } else {
            None
        };

        if file.records.len() > 5000 {
            let drop_n = file.records.len() - 5000;
            file.records.drain(0..drop_n);
        }
        drop(file);
        if changed.is_some() {
            self.persist_alarm_records()?;
        }
        Ok(changed)
    }

    pub fn record_notification(&self, record: NotificationRecord) -> anyhow::Result<()> {
        let mut file = self.notification_history.lock();
        file.records.push(record);
        if file.records.len() > 5000 {
            let drop_n = file.records.len() - 5000;
            file.records.drain(0..drop_n);
        }
        save_json(&self.notification_history_file, &*file)
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
            {
                let mut runtime = state.runtime_status.lock();
                for s in &sources {
                    let entry = runtime
                        .entry(s.id.clone())
                        .or_insert_with(|| ChannelRuntimeStatus::new(s.id.clone(), s.enabled, now));
                    entry.online_status = if s.enabled { "online" } else { "offline" }.into();
                    if s.enabled {
                        if entry.algorithm_status == "disabled" {
                            entry.algorithm_status = "idle".into();
                        }
                        entry.last_frame_at = Some(now);
                    } else {
                        entry.algorithm_status = "disabled".into();
                    }
                    entry.ts = now;
                }
                runtime.retain(|source_id, _| sources.iter().any(|s| &s.id == source_id));
            }
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
                            ((seed.wrapping_mul(9301).wrapping_add(49297) % 23328).unsigned_abs()
                                as u32)
                                + 800
                        } else {
                            0
                        },
                        fps: if online {
                            20 + ((seed.wrapping_mul(7) as u32) % 11)
                        } else {
                            0
                        },
                        viewers: if online {
                            (seed.wrapping_mul(13) as u32) % 8
                        } else {
                            0
                        },
                        location: s.location.clone(),
                        ts: now,
                    }
                })
                .collect();
            let _ = app.emit("ecoalert://status", &stats);
            let runtime = state.runtime_status_snapshot();
            let _ = app.emit("ecoalert://runtime_status", &runtime);
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
            let algorithm_config = state.algorithm_config.lock().clone();
            let now = chrono::Utc::now().timestamp_millis();
            for s in &sources {
                let decision = scheduler::decide_for_source(s, &algorithm_config);
                {
                    let mut runtime = state.runtime_status.lock();
                    let entry = runtime
                        .entry(s.id.clone())
                        .or_insert_with(|| ChannelRuntimeStatus::new(s.id.clone(), s.enabled, now));
                    entry.effective_algorithm_config_scope = decision.config_scope.clone();
                    entry.algorithm_status = if decision.should_run_simple {
                        "running".into()
                    } else {
                        "disabled".into()
                    };
                    entry.last_error = if decision.should_run_simple {
                        None
                    } else {
                        Some(decision.reason.clone())
                    };
                    entry.ts = now;
                }
                let _ = app.emit(
                    "ecoalert://algorithm_schedule",
                    serde_json::json!({
                        "source_id": s.id,
                        "action": if decision.should_run_simple { "run_simple" } else { "skip" },
                        "reason": decision.reason.clone(),
                        "latency_ms": null,
                        "ts": now,
                    }),
                );

                if !decision.should_run_simple {
                    continue;
                }
                let id_bytes = s.id.bytes().next().unwrap_or(0) as u64;
                let (counter, prev_p, prev_l) =
                    seeds.entry(s.id.clone()).or_insert((0, false, false));
                // 噪声：基于时间窗和源 id 的伪随机
                let t = (now / 4000).max(0) as u64;
                let n = id_bytes.wrapping_mul(t);
                let person = (n % 5) < 2; // ~40% 概率有人
                let light = (n.wrapping_mul(3) % 7) < 3; // ~43% 概率开灯
                *counter += 1;
                let new_state = SceneState {
                    person,
                    light,
                    frame_seq: *counter,
                    confidence: 0.75 + ((n % 50) as f32) / 200.0,
                };
                {
                    let mut current = state.current_state.lock();
                    current.insert(s.id.clone(), new_state.clone());
                }
                {
                    let mut runtime = state.runtime_status.lock();
                    let entry = runtime
                        .entry(s.id.clone())
                        .or_insert_with(|| ChannelRuntimeStatus::new(s.id.clone(), s.enabled, now));
                    entry.algorithm_status = "idle".into();
                    entry.last_error = None;
                    entry.last_algorithm_at = Some(now);
                    entry.alarm_status = if !person && light {
                        "alarm_active".into()
                    } else {
                        "normal".into()
                    };
                    entry.ts = now;
                }
                // 只在变化时落库 + 推送
                if person != *prev_p || light != *prev_l {
                    let rec = StateRecord::from_change(&s.id, &new_state);
                    let rec_id = rec.id.clone();
                    let alarm = rec.alarm;
                    if let Err(e) = state.record_state_change(rec) {
                        log::warn!("历史落库失败: {e}");
                    }
                    match state.apply_alarm_state(&s.id, alarm, Some(rec_id), now) {
                        Ok(Some(alarm_record)) => {
                            {
                                let mut runtime = state.runtime_status.lock();
                                if let Some(entry) = runtime.get_mut(&s.id) {
                                    entry.alarm_status = alarm_record.status.clone();
                                    entry.ts = now;
                                }
                            }
                            let _ = app.emit(
                                "ecoalert://alarm",
                                serde_json::json!({
                                    "alarm_id": alarm_record.id,
                                    "source_id": alarm_record.source_id,
                                    "status": alarm_record.status,
                                    "event": if alarm { "alarm_triggered" } else { "alarm_resolved" },
                                    "ts": now,
                                }),
                            );
                            let app_for_notify = app.clone();
                            let state_for_notify = state.inner().clone();
                            let event = if alarm {
                                "alarm_triggered"
                            } else {
                                "alarm_resolved"
                            }
                            .to_string();
                            tokio::spawn(async move {
                                notifier::dispatch_alarm_event(
                                    app_for_notify,
                                    state_for_notify,
                                    event,
                                    alarm_record,
                                )
                                .await;
                            });
                        }
                        Ok(None) => {}
                        Err(e) => log::warn!("报警状态落库失败: {e}"),
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
