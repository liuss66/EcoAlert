// 应用状态：登录态、视频源、分组、状态推送、算法接入点
use crate::auth::AuthConfig;
use crate::pipeline::vlm;
use crate::pipeline::{
    decoder::{extract_gray_frame_from_url_at, probe_media_duration_secs},
    detector::Detector,
    notifier, scheduler, yolo_detector, PipelineConfig,
};
use crate::store::{
    backfill_groups, load, load_history, load_json, migrate_local_hls_demo_names, save,
    save_history, save_json, AlarmRecord, AlarmRecordFile, AlgorithmConfig, AlgorithmConfigFile,
    ChannelRuntimeStatus, DataFile, DetectionHistoryFile, DetectionSampleRecord, HistoryFile,
    NotificationConfigFile, NotificationHistoryFile, NotificationRecord, RoiConfigFile, SceneState,
    SecurityConfig, SourceGroup, StateRecord, VideoSource,
};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const PERSON_PRESENCE_HOLD_MS: i64 = 5 * 60 * 1000;

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
        // 测试源由调试菜单"测试视频源"开关控制，启动时不再自动创建。
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
        migrate_algorithm_default_tuning(&mut algorithm_config);
        let mut roi_config: RoiConfigFile = load_json(&roi_config_file);
        roi_config.migrate_legacy_thresholds();
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

fn migrate_algorithm_default_tuning(config: &mut AlgorithmConfigFile) {
    fn migrate_one(cfg: &mut AlgorithmConfig) {
        // 仅迁移历史默认组合，避免覆盖用户手动设置为 10 秒的配置。
        if cfg.simple_interval_sec == 10 && (cfg.person_threshold - 0.006).abs() < 0.0005 {
            cfg.simple_interval_sec = 1;
        }
        if (cfg.person_threshold - 0.006).abs() < 0.0005 {
            cfg.person_threshold = 0.003;
        }
    }
    migrate_one(&mut config.global);
    for cfg in config.groups.values_mut() {
        migrate_one(cfg);
    }
    for cfg in config.sources.values_mut() {
        migrate_one(cfg);
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
        let mut domain_notified_groups: HashSet<String> = HashSet::new();
        let mut last_simple_run: HashMap<String, i64> = HashMap::new();
        let mut config_signatures: HashMap<String, String> = HashMap::new();
        let mut presence_trackers: HashMap<String, PresenceTracker> = HashMap::new();
        // YOLO 成功计数器：每源每 10 次成功打一条 INFO 日志，验证多路源都进了 YOLO
        let mut yolo_success_count: HashMap<String, u32> = HashMap::new();
        let mut yolo_logged_sources: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut vlm_hourly_usage: HashMap<String, (i64, u32)> = HashMap::new();
        let mut media_durations: HashMap<String, Option<f64>> = HashMap::new();
        // YOLO 检测：每源一个 WebSocket 客户端 + 熔断器
        // WS 长连接复用，避免每帧建连/拆连的开销
        let mut yolo_clients: HashMap<String, yolo_detector::YoloClient> = HashMap::new();
        let mut yolo_breakers: HashMap<String, yolo_detector::YoloCircuitBreaker> = HashMap::new();
        loop {
            interval.tick().await;
            let tick_start = std::time::Instant::now();
            let state = app.state::<Arc<AppState>>();
            let sources = state.sources.lock().clone();
            let algorithm_config = state.algorithm_config.lock().clone();
            let now = chrono::Utc::now().timestamp_millis();

            // ── Phase 1: 调度决策 + 并发派发所有源的抽帧任务 ──────────────
            // 旧版串行 .await 每路视频依次等待 ffmpeg，N 路源延迟线性累加。
            // 现在先批量 spawn_blocking，再统一等结果，总延迟 ≈ 最慢的那一路。
            // YOLO 模式：直接抽 1280x720 高分辨率帧，避免二次抽帧耗时
            struct PendingExtraction {
                source_idx: usize,
                source_id: String,
                source_enabled: bool,
                effective_config: AlgorithmConfig,
                should_run_vlm: bool,
                join_handle:
                    tokio::task::JoinHandle<anyhow::Result<crate::pipeline::decoder::DecodedFrame>>,
            }
            let mut pending: Vec<PendingExtraction> = Vec::new();

            for (idx, s) in sources.iter().enumerate() {
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
                    entry.yolo_enabled = decision.effective_config.yolo_enabled;
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
                let (counter, _prev_p, _prev_l) =
                    seeds.entry(s.id.clone()).or_insert((0, false, false));
                *counter += 1;
                let current_counter = *counter;
                let url = s.url.clone();
                let seek_secs = if s.source_type == "mp4" {
                    let duration = if let Some(cached) = media_durations.get(&s.id) {
                        *cached
                    } else {
                        let probed = probe_media_duration_secs(&s.url, Duration::from_secs(3))
                            .ok()
                            .flatten();
                        media_durations.insert(s.id.clone(), probed);
                        probed
                    };
                    let base = current_counter.saturating_sub(1) as f64;
                    Some(match duration {
                        Some(duration) if duration > 0.5 => base % duration,
                        _ => base,
                    })
                } else {
                    None
                };
                // YOLO 模式直接抽高分辨率帧，避免 Phase 3 二次抽帧
                let yolo_enabled = decision.effective_config.yolo_enabled;
                let (frame_width, frame_height) = if yolo_enabled {
                    (1280, 720)
                } else {
                    (320, 240)
                };
                let join_handle = tokio::task::spawn_blocking(move || {
                    let t0 = std::time::Instant::now();
                    let result = extract_gray_frame_from_url_at(
                        &url,
                        frame_width,
                        frame_height,
                        Duration::from_secs(5),
                        seek_secs,
                    );
                    let ms = t0.elapsed().as_millis();
                    if ms > 1500 {
                        log::warn!("[抽帧] {} {}x{} 耗时 {}ms (seek={:?})", url.chars().take(40).collect::<String>(), frame_width, frame_height, ms, seek_secs);
                    }
                    result
                });
                pending.push(PendingExtraction {
                    source_idx: idx,
                    source_id: s.id.clone(),
                    source_enabled: s.enabled,
                    effective_config: decision.effective_config.clone(),
                    should_run_vlm: decision.should_run_vlm,
                    join_handle,
                });
            }

            // ── Phase 2: 统一等待所有抽帧完成（并发，总延迟 ≈ max 单路） ──────
            struct ExtractionResult {
                source_idx: usize,
                effective_config: AlgorithmConfig,
                should_run_vlm: bool,
                frame: crate::pipeline::decoder::DecodedFrame,
            }
            let mut results: Vec<ExtractionResult> = Vec::new();
            for p in pending {
                match p.join_handle.await {
                    Ok(Ok(frame)) => {
                        results.push(ExtractionResult {
                            source_idx: p.source_idx,
                            effective_config: p.effective_config,
                            should_run_vlm: p.should_run_vlm,
                            frame,
                        });
                    }
                    Ok(Err(err)) => {
                        let msg = format!("真实帧抽取失败: {err}");
                        {
                            let mut runtime = state.runtime_status.lock();
                            let entry =
                                runtime.entry(p.source_id.clone()).or_insert_with(|| {
                                    ChannelRuntimeStatus::new(
                                        p.source_id.clone(),
                                        p.source_enabled,
                                        now,
                                    )
                                });
                            entry.vlm_enabled = p.effective_config.vlm_enabled;
                            entry.algorithm_status = "error".into();
                            entry.last_error = Some(msg.clone());
                            entry.ts = now;
                        }
                        let _ = app.emit(
                            "ecoalert://algorithm_schedule",
                            serde_json::json!({
                                "source_id": p.source_id,
                                "action": "frame_error",
                                "reason": msg,
                                "latency_ms": null,
                                "ts": now,
                            }),
                        );
                    }
                    Err(err) => {
                        let msg = format!("抽帧任务异常: {err}");
                        {
                            let mut runtime = state.runtime_status.lock();
                            let entry =
                                runtime.entry(p.source_id.clone()).or_insert_with(|| {
                                    ChannelRuntimeStatus::new(
                                        p.source_id.clone(),
                                        p.source_enabled,
                                        now,
                                    )
                                });
                            entry.vlm_enabled = p.effective_config.vlm_enabled;
                            entry.algorithm_status = "error".into();
                            entry.last_error = Some(msg.clone());
                            entry.ts = now;
                        }
                        log::warn!("{msg}");
                    }
                }
            }

            // ── Phase 3: 顺序处理检测结果（VLM / 报警 / 事件推送） ──────────
            for r in results {
                let s = &sources[r.source_idx];
                let decision_effective_config = r.effective_config;
                let frame = r.frame;
                let roi_config = state.roi_config.lock().effective_for_source(&s.id);
                let detector = detectors.entry(s.id.clone()).or_insert_with(|| {
                    Detector::with_light_threshold(
                        PipelineConfig::default(),
                        decision_effective_config.light_threshold,
                    )
                });
                // 每次循环更新灯光阈值，响应用户配置变更而不丢失 EMA 状态
                detector.set_light_threshold(decision_effective_config.light_threshold);
                let analysis = detector.analyze_scene(&frame, Some(&roi_config));
                let mut new_state = analysis.scene;

                // ── YOLO 人员检测（WebSocket 长连接） ──────────────────────
                let mut yolo_detections: Vec<yolo_detector::YoloDetection> = Vec::new();
                let mut yolo_error_msg: Option<String> = None;
                if decision_effective_config.yolo_enabled {
                    if yolo_logged_sources.insert(s.id.clone()) {
                        log::info!(
                            "[yolo] 通道 {} 首次进入 YOLO 检测分支 (api={}, conf={})",
                            s.id, decision_effective_config.yolo_api_base,
                            decision_effective_config.yolo_confidence
                        );
                    }
                    let breaker = yolo_breakers.entry(s.id.clone()).or_default();
                    if breaker.is_open(now) {
                        new_state.reason = Some("yolo_cooldown".into());
                        log::debug!("[yolo] {} 处于熔断冷却期，跳过检测", s.id);
                    } else {
                        // 配置或地址变化时重建连接
                        let client = yolo_clients
                            .entry(s.id.clone())
                            .or_insert_with(|| {
                                yolo_detector::YoloClient::new(
                                    decision_effective_config.yolo_api_base.clone(),
                                )
                            });
                        if client.api_base() != decision_effective_config.yolo_api_base {
                            *client = yolo_detector::YoloClient::new(
                                decision_effective_config.yolo_api_base.clone(),
                            );
                        }

                        // 直接使用 Phase 2 抽取的高分辨率帧（YOLO 模式下已抽 1280x720）
                        match yolo_detector::encode_frame_as_jpeg(&frame) {
                            Ok(jpeg_bytes) => {
                                // 顶层超时 7s：建连 + 发送 + 接收
                                let yolo_result = tokio::time::timeout(
                                    Duration::from_secs(7),
                                    client.detect_frame(jpeg_bytes),
                                )
                                .await;
                                match yolo_result {
                                    Ok(Ok(result)) => {
                                        breaker.record_success();
                                        let cnt = yolo_success_count
                                            .entry(s.id.clone())
                                            .or_insert(0);
                                        *cnt += 1;
                                        if *cnt % 10 == 1 {
                                            log::info!(
                                                "[yolo] {} 检测成功 person={} detections={} conf={:.2} latency={}ms (tick #{cnt})",
                                                s.id, result.person, result.detections.len(),
                                                result.person_confidence, result.process_ms
                                            );
                                        }
                                        new_state.person = result.person;
                                        new_state.person_confidence = result.person_confidence;
                                        new_state.source = "yolo".into();
                                        new_state.model_latency_ms =
                                            Some(result.process_ms as u32);
                                        yolo_detections = result.detections;
                                    }
                                    Ok(Err(e)) => {
                                        breaker.record_failure(now);
                                        log::error!("[yolo] {} 检测失败: {}", s.id, e);
                                        new_state.reason = Some(format!("yolo_error: {e}"));
                                        yolo_error_msg = Some(format!("YOLO 检测失败: {e}"));
                                    }
                                    Err(_) => {
                                        breaker.record_failure(now);
                                        log::error!("[yolo] {} 检测超时（>7s）", s.id);
                                        new_state.reason = Some("yolo_error: 调用超时".into());
                                        yolo_error_msg = Some("YOLO 检测超时（>7s）".into());
                                        // 超时主动断开，下次重建
                                        client.disconnect().await;
                                    }
                                }
                            }
                            Err(e) => {
                                log::error!("[yolo] {} 帧编码失败: {}", s.id, e);
                                new_state.reason = Some(format!("yolo_error: {e}"));
                                yolo_error_msg = Some(format!("YOLO 帧编码失败: {e}"));
                            }
                        }
                    }
                }
                let previous_yolo_error = {
                    state
                        .runtime_status
                        .lock()
                        .get(&s.id)
                        .and_then(|entry| entry.yolo_error.clone())
                };
                let yolo_failure_active = decision_effective_config.yolo_enabled
                    && yolo_error_msg.is_some();
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

                // 统计 VLM 每小时使用量
                let hour_bucket = now / 3_600_000;
                let usage_entry = vlm_hourly_usage
                    .entry(s.id.clone())
                    .or_insert((hour_bucket, 0));
                if usage_entry.0 != hour_bucket {
                    *usage_entry = (hour_bucket, 0);
                }
                let vlm_limit = decision_effective_config.vlm_hourly_limit;
                let vlm_under_limit = vlm_limit == 0 || usage_entry.1 < vlm_limit;
                let light_off_skip = !new_state.light;

                let person = new_state.person;
                let light = new_state.light;
                // 报警条件：灯亮且无人
                let raw_alarm = light && !person && !yolo_failure_active;
                let recover_condition =
                    should_recover_alarm(person, light, &decision_effective_config.recover_policy);
                let alarm_transition = alarm_timers.entry(s.id.clone()).or_default().update(
                    raw_alarm,
                    recover_condition,
                    now,
                    decision_effective_config.alarm_hold_sec,
                    decision_effective_config.alarm_recover_sec,
                );
                let alarm_countdown_progress = alarm_timers
                    .get(&s.id)
                    .map(|timer| {
                        timer.alarm_progress(now, decision_effective_config.alarm_hold_sec)
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
                        } else {
                            "normal"
                        }
                    })
                    .unwrap_or("normal");
                {
                    let mut current = state.current_state.lock();
                    current.insert(s.id.clone(), new_state.clone());
                }
                let mut vlm_error_msg: Option<String> = None;
                {
                    let mut runtime = state.runtime_status.lock();
                    let entry = runtime
                        .entry(s.id.clone())
                        .or_insert_with(|| ChannelRuntimeStatus::new(s.id.clone(), s.enabled, now));
                    entry.algorithm_status = "idle".into();
                    entry.vlm_enabled = decision_effective_config.vlm_enabled;
                    entry.yolo_enabled = decision_effective_config.yolo_enabled;
                    entry.yolo_error = yolo_error_msg.clone();
                    // YOLO 错误优先于 VLM 错误显示
                    entry.last_error = yolo_error_msg
                        .clone()
                        .or_else(|| vlm_error_msg.clone());
                    entry.last_algorithm_at = Some(now);
                    entry.alarm_status = alarm_status.into();
                    entry.ts = now;
                }
                let yolo_error_for_scene = yolo_error_msg.clone();
                let scene_payload = serde_json::json!({
                    "source_id": s.id,
                    "person": person,
                    "light": light,
                    "light_state": if light { "on" } else { "off" },
                    "alarm": alarm_status == "alarm_active",
                    "alarm_record_active": alarm_timers.get(&s.id).map(|timer| timer.active).unwrap_or(false),
                    "alarm_status": alarm_status,
                    "alarm_progress": alarm_countdown_progress,
                    "alarm_countdown_progress": alarm_countdown_progress,
                    "simple_person": simple_person,
                    "simple_person_confidence": simple_person_confidence,
                    "vlm_person": vlm_person,
                    "vlm_person_confidence": vlm_person_confidence,
                    "vlm_status": vlm_status,
                    "yolo_error": yolo_error_for_scene,
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
                    "yolo_detections": yolo_detections.iter().map(|d| {
                        serde_json::json!({ "confidence": d.confidence, "bbox": d.bbox })
                    }).collect::<Vec<_>>(),
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
                let state_changed = seeds
                    .get(&s.id)
                    .map(|(_, pp, pl)| person != *pp || light != *pl)
                    .unwrap_or(true);
                if state_changed {
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
                    if let Some(seed) = seeds.get_mut(&s.id) {
                        seed.1 = person;
                        seed.2 = light;
                    }
                }

                if let Some(alarm_active) = alarm_transition {
                    // 报警触发时，如果 VLM 启用，先调用 VLM 确认是否有人
                    if alarm_active && decision_effective_config.vlm_enabled && vlm_under_limit && !light_off_skip {
                        let vlm_started = chrono::Utc::now().timestamp_millis();
                        match vlm::analyze_person(&decision_effective_config, &frame).await {
                            Ok(vlm_result) => {
                                usage_entry.1 = usage_entry.1.saturating_add(1);
                                vlm_person = Some(vlm_result.has_person);
                                vlm_person_confidence = Some(vlm_result.confidence);
                                let latency = chrono::Utc::now()
                                    .timestamp_millis()
                                    .saturating_sub(vlm_started);

                                if vlm_result.has_person {
                                    // VLM 检测到人，取消报警，重置计时器
                                    vlm_status = "person_alarm_cancelled";
                                    log::info!(
                                        "[vlm] 报警确认：检测到人员，取消报警 source={} confidence={:.2} latency={}ms",
                                        s.id,
                                        vlm_result.confidence,
                                        latency
                                    );
                                    // 重置报警计时器
                                    if let Some(timer) = alarm_timers.get_mut(&s.id) {
                                        timer.reset();
                                    }
                                    // 不继续处理报警
                                    continue;
                                } else {
                                    // VLM 确认无人，正常报警
                                    vlm_status = "no_person_alarm_confirmed";
                                    log::info!(
                                        "[vlm] 报警确认：无人，继续报警 source={} confidence={:.2} latency={}ms",
                                        s.id,
                                        vlm_result.confidence,
                                        latency
                                    );
                                }
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
                            }
                            Err(err) => {
                                // VLM 调用失败，继续报警（按用户要求：失败则正常报警）
                                vlm_status = "error";
                                let msg = format!("VLM 检测失败: {err}");
                                log::warn!("[vlm] 报警确认失败，继续报警 source={} error={}", s.id, err);
                                vlm_error_msg = Some(msg.clone());
                            }
                        }
                    }

                    match state.apply_alarm_state(&s.id, alarm_active, state_record_id, now) {
                        Ok(Some(alarm_record)) => {
                            {
                                let mut runtime = state.runtime_status.lock();
                                if let Some(entry) = runtime.get_mut(&s.id) {

                    if yolo_failure_active && previous_yolo_error.is_none() {
                        let app_for_notify = app.clone();
                        let state_for_notify = state.inner().clone();
                        let source_id = s.id.clone();
                        let detail = yolo_error_msg.clone().unwrap_or_else(|| "YOLO 服务器失效".into());
                        tauri::async_runtime::spawn(async move {
                            notifier::dispatch_system_event(
                                app_for_notify,
                                state_for_notify,
                                "yolo_error".into(),
                                Some(source_id),
                                "YOLO 服务器失效".into(),
                                detail,
                            )
                            .await;
                        });
                    }
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
                            let event = if alarm_active {
                                "alarm_triggered"
                            } else {
                                "alarm_resolved"
                            }
                            .to_string();
                            if should_dispatch_alarm_notification(
                                &event,
                                &alarm_record.source_id,
                                state.inner(),
                                &mut domain_notified_groups,
                            ) {
                                let app_for_notify = app.clone();
                                let state_for_notify = state.inner().clone();
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
                        }
                        Ok(None) => {}
                        Err(e) => log::warn!("报警状态落库失败: {e}"),
                    }
                }
            }
            let tick_ms = tick_start.elapsed().as_millis();
            if tick_ms > 1500 {
                log::warn!("[tick] 本轮耗时 {}ms，源数量={}", tick_ms, sources.len());
            }
        }
    });
}

fn should_dispatch_alarm_notification(
    event: &str,
    source_id: &str,
    state: &Arc<AppState>,
    domain_notified_groups: &mut HashSet<String>,
) -> bool {
    let sources = state.sources.lock().clone();
    let Some(source) = sources.iter().find(|item| item.id == source_id) else {
        return true;
    };
    let Some(group_id) = source.group_id.as_deref() else {
        return true;
    };
    let groups = state.groups.lock().clone();
    let Some(group) = groups.iter().find(|item| item.id == group_id) else {
        return true;
    };
    if !group.domain_detection_enabled {
        return true;
    }

    let group_sources: Vec<&VideoSource> = sources
        .iter()
        .filter(|item| item.enabled && item.group_id.as_deref() == Some(group_id))
        .collect();
    if group_sources.is_empty() {
        return false;
    }

    let runtime = state.runtime_status.lock();
    let all_group_sources_alarm = group_sources.iter().all(|item| {
        runtime
            .get(&item.id)
            .map(|status| {
                matches!(
                    status.alarm_status.as_str(),
                    "alarm_active" | "acknowledged" | "recovering"
                )
            })
            .unwrap_or(false)
    });
    drop(runtime);

    match event {
        "alarm_triggered" => {
            if all_group_sources_alarm && !domain_notified_groups.contains(group_id) {
                domain_notified_groups.insert(group_id.to_string());
                true
            } else {
                false
            }
        }
        "alarm_resolved" => {
            if domain_notified_groups.contains(group_id) && !all_group_sources_alarm {
                domain_notified_groups.remove(group_id);
                true
            } else {
                false
            }
        }
        _ => true,
    }
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

    /// 重置报警计时器（VLM 确认有人时调用）
    fn reset(&mut self) {
        self.active = false;
        self.alarm_since = None;
        self.recover_since = None;
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
        "enabled={};dev={};simple={};person={:.5};hold={};recover={};policy={};vlm={};vlm_interval={};vlm_skip={};vlm_limit={};yolo={};yolo_api={};yolo_conf={:.3}",
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
        config.yolo_enabled,
        config.yolo_api_base,
        config.yolo_confidence,
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
    use super::{migrate_algorithm_default_tuning, should_recover_alarm, AlarmTimer};
    use crate::store::AlgorithmConfigFile;

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

    #[test]
    fn migrate_default_tuning_updates_legacy_defaults() {
        let mut file = AlgorithmConfigFile::default();
        file.global.simple_interval_sec = 10;
        file.global.person_threshold = 0.006;

        migrate_algorithm_default_tuning(&mut file);

        assert_eq!(file.global.simple_interval_sec, 1);
        assert!((file.global.person_threshold - 0.003).abs() < 0.0001);
    }

    #[test]
    fn migrate_default_tuning_keeps_user_interval_ten() {
        let mut file = AlgorithmConfigFile::default();
        file.global.simple_interval_sec = 10;
        file.global.person_threshold = 0.003;

        migrate_algorithm_default_tuning(&mut file);

        assert_eq!(file.global.simple_interval_sec, 10);
        assert!((file.global.person_threshold - 0.003).abs() < 0.0001);
    }
}
