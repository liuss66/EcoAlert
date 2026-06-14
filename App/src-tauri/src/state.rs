// 应用状态：登录态、视频源、分组、状态推送、算法接入点
use crate::auth::AuthConfig;
use crate::pipeline::vlm;
use crate::pipeline::{
    decoder::extract_gray_frame_from_url, detector::Detector, notifier, scheduler, PipelineConfig,
};
use crate::store::{
    backfill_groups, load, load_history, load_json, migrate_local_hls_demo_names, save,
    save_history, save_json, seed_local_hls_sources_if_empty, AlarmRecord, AlarmRecordFile,
    AlgorithmConfig, AlgorithmConfigFile, ChannelRuntimeStatus, DataFile, DetectionHistoryFile,
    DetectionSampleRecord, HistoryFile, NotificationConfigFile, NotificationHistoryFile,
    NotificationRecord, RoiConfigFile, SceneState, SecurityConfig, SourceGroup, StateRecord,
    VideoSource,
};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const PERSON_PRESENCE_HOLD_MS: i64 = 5 * 60 * 1000;
const VLM_NO_PERSON_CONFIRMATIONS: u8 = 2;

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
    pub detection_history_file: PathBuf,
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
    pub detection_history: Mutex<DetectionHistoryFile>,
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
        let detection_history_file = data_dir.join("detection_history.json");
        let algorithm_config_file = data_dir.join("algorithm_config.json");
        let roi_config_file = data_dir.join("roi_config.json");
        let notification_config_file = data_dir.join("notification_config.json");
        let security_config_file = data_dir.join("security_config.json");
        let alarm_records_file = data_dir.join("alarm_records.json");
        let notification_history_file = data_dir.join("notification_history.json");

        // 加载数据
        let mut data = load(&data_file);
        // 首次运行预置本地 HLS 测试源，匹配 Tools/push_streamer 输出。
        seed_local_hls_sources_if_empty(&mut data);
        // 修正旧版内置演示源命名，只影响本地 HLS 默认源。
        migrate_local_hls_demo_names(&mut data);
        // 向前兼容：补全 group_id
        backfill_groups(&mut data);
        // 首次运行把补全后的结果落盘
        if let Err(e) = save(&data_file, &data) {
            log::warn!("初始化落盘失败(sources.json): {e}");
        }

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
        let detection_history: DetectionHistoryFile = load_json(&detection_history_file);
        let mut algorithm_config: AlgorithmConfigFile = load_json(&algorithm_config_file);
        migrate_legacy_default_algorithm_window(&mut algorithm_config);
        let mut roi_config: RoiConfigFile = load_json(&roi_config_file);
        migrate_manual_source_config_mode(&mut algorithm_config, &mut roi_config);
        let notification_config: NotificationConfigFile = load_json(&notification_config_file);
        let security_config: SecurityConfig = load_json(&security_config_file);
        let alarm_records: AlarmRecordFile = load_json(&alarm_records_file);
        let notification_history: NotificationHistoryFile = load_json(&notification_history_file);
        if let Err(e) = save_json(&algorithm_config_file, &algorithm_config) {
            log::warn!("初始化落盘失败(algorithm_config.json): {e}");
        }
        if let Err(e) = save_json(&roi_config_file, &roi_config) {
            log::warn!("初始化落盘失败(roi_config.json): {e}");
        }
        if let Err(e) = save_json(&notification_config_file, &notification_config) {
            log::warn!("初始化落盘失败(notification_config.json): {e}");
        }
        if let Err(e) = save_json(&security_config_file, &security_config) {
            log::warn!("初始化落盘失败(security_config.json): {e}");
        }
        if let Err(e) = save_json(&alarm_records_file, &alarm_records) {
            log::warn!("初始化落盘失败(alarm_records.json): {e}");
        }
        if let Err(e) = save_json(&notification_history_file, &notification_history) {
            log::warn!("初始化落盘失败(notification_history.json): {e}");
        }
        if let Err(e) = save_json(&detection_history_file, &detection_history) {
            log::warn!("初始化落盘失败(detection_history.json): {e}");
        }
        let sources = data.sources.clone();
        let groups = data.groups.clone();

        Ok(Arc::new(Self {
            data_dir,
            data_file,
            auth_file,
            history_file,
            detection_history_file,
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
            detection_history: Mutex::new(detection_history),
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
        h.records.push_back(rec);
        // 简单的总量截断：保留最近 5000 条
        while h.records.len() > 5000 {
            h.records.pop_front();
        }
        save_history(&self.history_file, &h)
    }

    /// 详细检测采样落盘：开发者诊断曲线使用，每个源最多保留 2000 条，总量 20000 条。
    pub fn record_detection_sample(&self, rec: DetectionSampleRecord) -> anyhow::Result<()> {
        let source_id = rec.source_id.clone();
        let mut h = self.detection_history.lock();
        h.records.push_back(rec);

        let mut source_count = h
            .records
            .iter()
            .filter(|item| item.source_id == source_id)
            .count();
        while source_count > 2000 {
            if let Some(idx) = h
                .records
                .iter()
                .position(|item| item.source_id == source_id)
            {
                h.records.remove(idx);
                source_count -= 1;
            } else {
                break;
            }
        }
        while h.records.len() > 20000 {
            h.records.pop_front();
        }
        save_json(&self.detection_history_file, &*h)
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
                file.records.push_back(record.clone());
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
            while file.records.len() > 5000 {
                file.records.pop_front();
            }
        }
        drop(file);
        if changed.is_some() {
            self.persist_alarm_records()?;
        }
        Ok(changed)
    }

    pub fn record_notification(&self, record: NotificationRecord) -> anyhow::Result<()> {
        let mut file = self.notification_history.lock();
        file.records.push_back(record);
        while file.records.len() > 5000 {
            file.records.pop_front();
        }
        save_json(&self.notification_history_file, &*file)
    }
}

fn migrate_legacy_default_algorithm_window(config: &mut AlgorithmConfigFile) {
    let windows = &config.global.active_windows;
    let is_legacy_default = windows.len() == 1
        && windows[0].weekdays == vec![1, 2, 3, 4, 5]
        && windows[0].start == "18:30"
        && windows[0].end == "08:30"
        && config.global.exception_windows.is_empty();
    if is_legacy_default {
        config.global.active_windows.clear();
    }
}

fn migrate_manual_source_config_mode(
    algorithm_config: &mut AlgorithmConfigFile,
    roi_config: &mut RoiConfigFile,
) {
    if !algorithm_config.manual_source_config_mode {
        algorithm_config.groups.clear();
        algorithm_config.sources.clear();
        algorithm_config.manual_source_config_mode = true;
    }
    if !roi_config.manual_source_config_mode {
        roi_config.by_source.clear();
        roi_config.manual_source_config_mode = true;
    }
}

/// 启动后台状态推送任务（码率/FPS/在线）
pub fn spawn_status_ticker(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
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

/// 启动算法推送任务（ffmpeg 按需抽帧 + 轻量检测）
///
/// 算法接口契约（请勿修改）：
///   输入：每路视频的连续帧（pipeline 内部完成解码）
///   输出：SceneState { person, light, confidence, brightness, motion_score, ... }
///   推送：每次检测完成后通过 Tauri event "ecoalert://scene_state" 发给前端
///   落库：状态变化时调用 record_state_change
pub fn spawn_scene_state_ticker(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        // 每个源保留一个 Detector，确保 EMA / 帧差状态连续。
        let mut detectors: HashMap<String, Detector> = HashMap::new();
        let mut seeds: HashMap<String, (u64, bool, bool)> = HashMap::new();
        let mut alarm_timers: HashMap<String, AlarmTimer> = HashMap::new();
        let mut last_simple_run: HashMap<String, i64> = HashMap::new();
        let mut config_signatures: HashMap<String, String> = HashMap::new();
        let mut last_vlm_run: HashMap<String, i64> = HashMap::new();
        let mut presence_trackers: HashMap<String, PresenceTracker> = HashMap::new();
        let mut vlm_hourly_usage: HashMap<String, (i64, u32)> = HashMap::new();
        loop {
            interval.tick().await;
            let state = app.state::<Arc<AppState>>();
            let sources = state.sources.lock().clone();
            let algorithm_config = state.algorithm_config.lock().clone();
            let now = chrono::Utc::now().timestamp_millis();
            for s in &sources {
                let decision = scheduler::decide_for_source(s, &algorithm_config);
                let config_signature = algorithm_config_signature(&decision.effective_config);
                let config_changed = config_signatures
                    .get(&s.id)
                    .map(|prev| prev != &config_signature)
                    .unwrap_or(true);
                let simple_interval_ms =
                    decision.effective_config.simple_interval_sec.max(1) as i64 * 1000;
                let last_run = last_simple_run.get(&s.id).copied();
                let should_wait_interval = decision.should_run_simple
                    && !config_changed
                    && last_run
                        .map(|last| now.saturating_sub(last) < simple_interval_ms)
                        .unwrap_or(false);
                {
                    let mut runtime = state.runtime_status.lock();
                    let entry = runtime
                        .entry(s.id.clone())
                        .or_insert_with(|| ChannelRuntimeStatus::new(s.id.clone(), s.enabled, now));
                    entry.effective_algorithm_config_scope = decision.config_scope.clone();
                    entry.vlm_enabled = decision.effective_config.vlm_enabled;
                    entry.algorithm_status = if should_wait_interval {
                        "idle".into()
                    } else if decision.should_run_simple {
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
                        "action": if should_wait_interval {
                            "skip"
                        } else if decision.should_run_simple {
                            "run_simple"
                        } else {
                            "skip"
                        },
                        "reason": if should_wait_interval {
                            "simple_interval_wait"
                        } else {
                            decision.reason.as_str()
                        },
                        "latency_ms": null,
                        "ts": now,
                    }),
                );

                if !decision.should_run_simple || should_wait_interval {
                    continue;
                }
                config_signatures.insert(s.id.clone(), config_signature);
                last_simple_run.insert(s.id.clone(), now);
                let (counter, prev_p, prev_l) =
                    seeds.entry(s.id.clone()).or_insert((0, false, false));
                *counter += 1;
                let url = s.url.clone();
                let source_id = s.id.clone();
                let source_enabled = s.enabled;
                let frame = match tokio::task::spawn_blocking(move || {
                    extract_gray_frame_from_url(&url, 160, 120, Duration::from_secs(5))
                })
                .await
                {
                    Ok(Ok(frame)) => frame,
                    Ok(Err(err)) => {
                        let msg = format!("真实帧抽取失败: {err}");
                        {
                            let mut runtime = state.runtime_status.lock();
                            let entry = runtime.entry(source_id.clone()).or_insert_with(|| {
                                ChannelRuntimeStatus::new(source_id.clone(), source_enabled, now)
                            });
                            entry.vlm_enabled = decision.effective_config.vlm_enabled;
                            entry.algorithm_status = "error".into();
                            entry.last_error = Some(msg.clone());
                            entry.ts = now;
                        }
                        let _ = app.emit(
                            "ecoalert://algorithm_schedule",
                            serde_json::json!({
                                "source_id": source_id,
                                "action": "frame_error",
                                "reason": msg,
                                "latency_ms": null,
                                "ts": now,
                            }),
                        );
                        continue;
                    }
                    Err(err) => {
                        let msg = format!("抽帧任务异常: {err}");
                        {
                            let mut runtime = state.runtime_status.lock();
                            let entry = runtime.entry(source_id.clone()).or_insert_with(|| {
                                ChannelRuntimeStatus::new(source_id.clone(), source_enabled, now)
                            });
                            entry.vlm_enabled = decision.effective_config.vlm_enabled;
                            entry.algorithm_status = "error".into();
                            entry.last_error = Some(msg.clone());
                            entry.ts = now;
                        }
                        log::warn!("{msg}");
                        continue;
                    }
                };
                let roi_config = state.roi_config.lock().effective_for_source(&s.id);
                let detector = detectors.entry(s.id.clone()).or_insert_with(|| {
                    Detector::with_thresholds(
                        PipelineConfig::default(),
                        decision.effective_config.person_threshold,
                        decision.effective_config.light_threshold,
                    )
                });
                // 每次循环更新阈值，响应用户配置变更而不丢失 EMA 状态
                detector.set_thresholds(
                    decision.effective_config.person_threshold,
                    decision.effective_config.light_threshold,
                );
                let analysis = detector.analyze_scene(&frame, Some(&roi_config));
                let mut new_state = analysis.scene;
                let tracker = presence_trackers.entry(s.id.clone()).or_default();
                let simple_person = new_state.person;
                let simple_person_confidence = new_state.person_confidence;
                let mut vlm_person: Option<bool> = None;
                let mut vlm_person_confidence: Option<f32> = None;
                let mut vlm_status = "none";
                if simple_person {
                    tracker.mark_person_seen(now);
                    new_state.reason = Some(append_reason(
                        new_state.reason.as_deref(),
                        "simple_person_hold_reset",
                    ));
                } else {
                    tracker.mark_simple_no_person(now);
                    if tracker.person_is_held(now) {
                        new_state.person = true;
                        new_state.person_confidence = new_state.person_confidence.max(0.55);
                        new_state.reason = Some(append_reason(
                            new_state.reason.as_deref(),
                            "person_hold_5min",
                        ));
                    }
                }

                let vlm_interval_ms =
                    decision.effective_config.vlm_interval_sec.max(30) as i64 * 1000;
                let vlm_last_run = last_vlm_run.get(&s.id).copied();
                let vlm_interval_ready = vlm_last_run
                    .map(|last| now.saturating_sub(last) >= vlm_interval_ms)
                    .unwrap_or(true);
                let hour_bucket = now / 3_600_000;
                let usage_entry = vlm_hourly_usage
                    .entry(s.id.clone())
                    .or_insert((hour_bucket, 0));
                if usage_entry.0 != hour_bucket {
                    *usage_entry = (hour_bucket, 0);
                }
                let vlm_limit = decision.effective_config.vlm_hourly_limit;
                let vlm_under_limit = vlm_limit == 0 || usage_entry.1 < vlm_limit;
                let needs_vlm_no_person_confirmation =
                    tracker.no_person_for_at_least(now, PERSON_PRESENCE_HOLD_MS);
                let vlm_skip_when_person = decision.effective_config.vlm_skip_when_person;
                let light_off_skip = !new_state.light;
                let mut vlm_skip_reason: Option<&'static str> = None;
                let should_run_vlm = decision.should_run_vlm
                    && vlm_interval_ready
                    && vlm_under_limit
                    && needs_vlm_no_person_confirmation
                    && {
                        if vlm_skip_when_person && simple_person {
                            vlm_skip_reason = Some("simple_person_detected");
                            false
                        } else if light_off_skip {
                            vlm_skip_reason = Some("light_off_no_alarm_risk");
                            false
                        } else {
                            true
                        }
                    };
                let mut vlm_error_msg: Option<String> = None;
                if !should_run_vlm {
                    if let Some(reason) = vlm_skip_reason {
                        vlm_status = "skipped";
                        let _ = app.emit(
                            "ecoalert://algorithm_schedule",
                            serde_json::json!({
                                "source_id": s.id,
                                "action": "skip_vlm",
                                "reason": reason,
                                "latency_ms": null,
                                "ts": now,
                            }),
                        );
                        log::debug!(
                            "VLM skipped source={} reason={} (skip_when_person={}, light_off={}, simple_person={})",
                            s.id,
                            reason,
                            vlm_skip_when_person,
                            light_off_skip,
                            simple_person,
                        );
                    }
                }
                if should_run_vlm {
                    let vlm_started = chrono::Utc::now().timestamp_millis();
                    match vlm::analyze_person(&decision.effective_config, &frame).await {
                        Ok(vlm_result) => {
                            last_vlm_run.insert(s.id.clone(), now);
                            usage_entry.1 = usage_entry.1.saturating_add(1);
                            vlm_person = Some(vlm_result.has_person);
                            vlm_person_confidence = Some(vlm_result.confidence);
                            if vlm_result.has_person {
                                vlm_status = "person";
                                tracker.mark_person_seen(now);
                                new_state.person = true;
                                new_state.person_confidence =
                                    new_state.person_confidence.max(vlm_result.confidence);
                                new_state.confidence =
                                    new_state.confidence.max(vlm_result.confidence);
                                new_state.source = "fused".into();
                                new_state.reason = Some("vlm_person_detected".into());
                            } else {
                                vlm_status = "no_person";
                                tracker.mark_vlm_no_person();
                                new_state.person = false;
                                new_state.person_confidence = 0.0;
                                new_state.source = "fused".into();
                                new_state.reason = Some(format!(
                                    "vlm_no_person_confirmed_{}/{}",
                                    tracker.vlm_no_person_streak, VLM_NO_PERSON_CONFIRMATIONS
                                ));
                            }
                            let latency = chrono::Utc::now()
                                .timestamp_millis()
                                .saturating_sub(vlm_started);
                            new_state.model_latency_ms = Some(latency as u32);
                            let _ = app.emit(
                                "ecoalert://algorithm_schedule",
                                serde_json::json!({
                                    "source_id": s.id,
                                    "action": "run_vlm",
                                    "reason": if vlm_result.has_person { "vlm_person_detected" } else { "vlm_no_person" },
                                    "latency_ms": latency,
                                    "ts": now,
                                }),
                            );
                            log::debug!(
                                "VLM result source={} person={} confidence={} raw={}",
                                s.id,
                                vlm_result.has_person,
                                vlm_result.confidence,
                                vlm_result.raw
                            );
                        }
                        Err(err) => {
                            last_vlm_run.insert(s.id.clone(), now);
                            vlm_status = "error";
                            let msg = format!("VLM 检测失败: {err}");
                            vlm_error_msg = Some(msg.clone());
                            {
                                let mut runtime = state.runtime_status.lock();
                                let entry = runtime.entry(s.id.clone()).or_insert_with(|| {
                                    ChannelRuntimeStatus::new(s.id.clone(), s.enabled, now)
                                });
                                entry.vlm_enabled = decision.effective_config.vlm_enabled;
                                entry.last_error = Some(msg.clone());
                                entry.ts = now;
                            }
                            let _ = app.emit(
                                "ecoalert://algorithm_schedule",
                                serde_json::json!({
                                    "source_id": s.id,
                                    "action": "vlm_error",
                                    "reason": msg,
                                    "latency_ms": null,
                                    "ts": now,
                                }),
                            );
                        }
                    }
                }
                if !tracker.person_is_held(now) && !simple_person {
                    new_state.person = false;
                }
                let person = new_state.person;
                let light = new_state.light;
                let vlm_confirmed_no_person =
                    tracker.vlm_no_person_streak >= VLM_NO_PERSON_CONFIRMATIONS;
                let raw_alarm = light && !person && vlm_confirmed_no_person;
                let vlm_progress = if light && !person && decision.should_run_vlm {
                    (tracker.vlm_no_person_streak as f32 / VLM_NO_PERSON_CONFIRMATIONS as f32)
                        .clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let recover_condition =
                    should_recover_alarm(person, light, &decision.effective_config.recover_policy);
                let alarm_transition = alarm_timers.entry(s.id.clone()).or_default().update(
                    raw_alarm,
                    recover_condition,
                    now,
                    decision.effective_config.alarm_hold_sec,
                    decision.effective_config.alarm_recover_sec,
                );
                let alarm_countdown_progress = alarm_timers
                    .get(&s.id)
                    .map(|timer| {
                        timer.alarm_progress(now, decision.effective_config.alarm_hold_sec)
                    })
                    .unwrap_or(0.0);
                let alarm_status = alarm_timers
                    .get(&s.id)
                    .map(|timer| {
                        if timer.active {
                            if recover_condition {
                                "recovering"
                            } else {
                                "alarm_active"
                            }
                        } else if raw_alarm {
                            "suspected"
                        } else if light
                            && !person
                            && needs_vlm_no_person_confirmation
                            && decision.should_run_vlm
                        {
                            "vlm_checking"
                        } else {
                            "normal"
                        }
                    })
                    .unwrap_or("normal");
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
                    entry.vlm_enabled = decision.effective_config.vlm_enabled;
                    entry.last_error = vlm_error_msg.clone();
                    entry.last_algorithm_at = Some(now);
                    entry.alarm_status = alarm_status.into();
                    entry.ts = now;
                }
                let scene_payload = serde_json::json!({
                    "source_id": s.id,
                    "person": person,
                    "light": light,
                    "light_state": if light { "on" } else { "off" },
                    "alarm": alarm_status == "alarm_active",
                    "alarm_record_active": alarm_timers.get(&s.id).map(|timer| timer.active).unwrap_or(false),
                    "alarm_status": alarm_status,
                    "alarm_progress": if raw_alarm { alarm_countdown_progress } else { vlm_progress },
                    "vlm_progress": vlm_progress,
                    "alarm_countdown_progress": alarm_countdown_progress,
                    "simple_person": simple_person,
                    "simple_person_confidence": simple_person_confidence,
                    "vlm_person": vlm_person,
                    "vlm_person_confidence": vlm_person_confidence,
                    "vlm_status": vlm_status,
                    "person_confidence": new_state.person_confidence,
                    "light_confidence": new_state.light_confidence,
                    "confidence": new_state.confidence,
                    "source": new_state.source,
                    "reason": new_state.reason,
                    "frame_seq": new_state.frame_seq,
                    "model_latency_ms": new_state.model_latency_ms,
                    "light_brightness": new_state.light_brightness,
                    "color_score": new_state.color_score,
                    "motion_score": new_state.motion_score,
                    "process_ms": new_state.process_ms,
                    "ts": now,
                });
                let _ = app.emit("ecoalert://scene_state", scene_payload);

                if let Err(e) = state.record_detection_sample(DetectionSampleRecord::from_scene(
                    &s.id,
                    &new_state,
                    alarm_status,
                    now,
                )) {
                    log::warn!("详细检测历史落库失败: {e}");
                }

                // 状态历史只在 person / light 变化时落库，避免历史文件快速膨胀。
                let mut state_record_id: Option<String> = None;
                if person != *prev_p || light != *prev_l {
                    let rec = StateRecord::from_change(&s.id, &new_state);
                    let rec_id = rec.id.clone();
                    if let Err(e) = state.record_state_change(rec) {
                        log::warn!("历史落库失败: {e}");
                    }
                    state_record_id = Some(rec_id);
                    log_event(
                        &app,
                        if raw_alarm { "warn" } else { "info" },
                        format!(
                            "[{}] 状态变化: 人={}(conf={:.2}) 灯={} 亮度={:.1} 色彩={:.3} 运动={:.3} 耗时={:.2}ms src={}{}",
                            s.name,
                            if person { "●" } else { "○" },
                            new_state.person_confidence,
                            if light { "●" } else { "○" },
                            analysis.light_brightness,
                            new_state.color_score,
                            analysis.motion_score,
                            analysis.process_ms,
                            new_state.source.as_str(),
                            if raw_alarm {
                                "  疑似无人亮灯"
                            } else {
                                ""
                            }
                        ),
                    );
                    *prev_p = person;
                    *prev_l = light;
                }

                if let Some(alarm_active) = alarm_transition {
                    match state.apply_alarm_state(&s.id, alarm_active, state_record_id, now) {
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
                                    "event": if alarm_active { "alarm_triggered" } else { "alarm_resolved" },
                                    "ts": now,
                                }),
                            );
                            let app_for_notify = app.clone();
                            let state_for_notify = state.inner().clone();
                            let event = if alarm_active {
                                "alarm_triggered"
                            } else {
                                "alarm_resolved"
                            }
                            .to_string();
                            tauri::async_runtime::spawn(async move {
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
                }
            }
        }
    });
}

#[derive(Default)]
struct PresenceTracker {
    last_person_seen_at: Option<i64>,
    first_no_person_at: Option<i64>,
    vlm_no_person_streak: u8,
}

impl PresenceTracker {
    fn mark_person_seen(&mut self, now: i64) {
        self.last_person_seen_at = Some(now);
        self.first_no_person_at = None;
        self.vlm_no_person_streak = 0;
    }

    fn mark_simple_no_person(&mut self, now: i64) {
        if self.last_person_seen_at.is_none() {
            self.first_no_person_at.get_or_insert(now);
        }
    }

    fn mark_vlm_no_person(&mut self) {
        self.vlm_no_person_streak = self.vlm_no_person_streak.saturating_add(1);
    }

    fn person_is_held(&self, now: i64) -> bool {
        self.last_person_seen_at
            .map(|seen_at| now.saturating_sub(seen_at) < PERSON_PRESENCE_HOLD_MS)
            .unwrap_or(false)
    }

    fn no_person_since(&self) -> Option<i64> {
        self.last_person_seen_at.or(self.first_no_person_at)
    }

    fn no_person_for_at_least(&self, now: i64, duration_ms: i64) -> bool {
        !self.person_is_held(now)
            && self
                .no_person_since()
                .map(|since| now.saturating_sub(since) >= duration_ms)
                .unwrap_or(false)
    }
}

fn append_reason(existing: Option<&str>, item: &str) -> String {
    match existing {
        Some(text) if !text.is_empty() => format!("{text};{item}"),
        _ => item.to_string(),
    }
}

#[derive(Default)]
struct AlarmTimer {
    alarm_since: Option<i64>,
    recover_since: Option<i64>,
    active: bool,
}

impl AlarmTimer {
    fn update(
        &mut self,
        raw_alarm: bool,
        recover_condition: bool,
        now: i64,
        hold_sec: u32,
        recover_sec: u32,
    ) -> Option<bool> {
        if raw_alarm {
            self.recover_since = None;
            let since = *self.alarm_since.get_or_insert(now);
            if !self.active && now - since >= hold_sec as i64 * 1000 {
                self.active = true;
                return Some(true);
            }
            return None;
        }

        self.alarm_since = None;
        if self.active {
            if recover_condition {
                let since = *self.recover_since.get_or_insert(now);
                if now - since >= recover_sec as i64 * 1000 {
                    self.active = false;
                    self.recover_since = None;
                    return Some(false);
                }
            } else {
                self.recover_since = None;
            }
        } else {
            self.recover_since = None;
        }
        None
    }

    fn alarm_progress(&self, now: i64, hold_sec: u32) -> f32 {
        if self.active {
            return 1.0;
        }
        let Some(since) = self.alarm_since else {
            return 0.0;
        };
        let hold_ms = (hold_sec as i64).saturating_mul(1000);
        if hold_ms <= 0 {
            return 1.0;
        }
        (now.saturating_sub(since) as f32 / hold_ms as f32).clamp(0.0, 1.0)
    }
}

fn should_recover_alarm(person: bool, light: bool, policy: &str) -> bool {
    match policy {
        "light_off" => !light,
        "person_present" => person,
        "both" => person && !light,
        "either" => person || !light,
        _ => person || !light,
    }
}

fn algorithm_config_signature(config: &AlgorithmConfig) -> String {
    format!(
        "enabled={};dev={};simple={};person={:.5};hold={};recover={};policy={};vlm={};vlm_interval={};vlm_skip={};vlm_limit={}",
        config.enabled,
        config.developer_mode,
        config.simple_interval_sec,
        normalized_person_threshold(config.person_threshold),
        config.alarm_hold_sec,
        config.alarm_recover_sec,
        config.recover_policy,
        config.vlm_enabled,
        config.vlm_interval_sec,
        config.vlm_skip_when_person,
        config.vlm_hourly_limit,
    )
}

fn normalized_person_threshold(value: f32) -> f32 {
    if value > 0.2 {
        (value.clamp(0.05, 1.0) * 0.03).max(0.001)
    } else {
        value.clamp(0.001, 0.20)
    }
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

#[cfg(test)]
mod tests {
    use super::{should_recover_alarm, AlarmTimer};

    #[test]
    fn alarm_timer_respects_hold_and_recover_seconds() {
        let mut timer = AlarmTimer::default();
        assert_eq!(timer.update(true, false, 1_000, 10, 5), None);
        assert_eq!(timer.update(true, false, 10_999, 10, 5), None);
        assert_eq!(timer.update(true, false, 11_000, 10, 5), Some(true));
        assert!(timer.active);

        assert_eq!(timer.update(false, true, 12_000, 10, 5), None);
        assert_eq!(timer.update(false, true, 16_999, 10, 5), None);
        assert_eq!(timer.update(false, true, 17_000, 10, 5), Some(false));
        assert!(!timer.active);
    }

    #[test]
    fn alarm_timer_resets_recover_window_when_condition_breaks() {
        let mut timer = AlarmTimer {
            active: true,
            ..AlarmTimer::default()
        };
        assert_eq!(timer.update(false, true, 1_000, 0, 5), None);
        assert_eq!(timer.update(false, false, 4_000, 0, 5), None);
        assert_eq!(timer.update(false, true, 6_000, 0, 5), None);
        assert_eq!(timer.update(false, true, 11_000, 0, 5), Some(false));
    }

    #[test]
    fn recover_policy_matches_config_values() {
        assert!(should_recover_alarm(false, false, "light_off"));
        assert!(!should_recover_alarm(true, true, "light_off"));
        assert!(should_recover_alarm(true, true, "person_present"));
        assert!(!should_recover_alarm(false, false, "person_present"));
        assert!(should_recover_alarm(true, false, "both"));
        assert!(!should_recover_alarm(true, true, "both"));
        assert!(should_recover_alarm(false, false, "either"));
        assert!(should_recover_alarm(true, true, "either"));
    }
}
