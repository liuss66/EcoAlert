// 应用状态：登录态、视频源、分组、状态推送、算法接入点
use crate::auth::AuthConfig;
use crate::pipeline::vlm;
use crate::pipeline::{
    decoder::{
        extract_gray_frame_from_url_at, extract_original_frame_from_url_at,
        probe_media_duration_secs,
    },
    detector::Detector,
    notifier, scheduler, yolo_detector, PipelineConfig,
};
use crate::store::{
    backfill_groups, load, load_history, load_json, migrate_local_hls_demo_names, save,
    save_history, save_json, save_json_compact, AlarmRecord, AlarmRecordFile, AlgorithmConfig,
    AlgorithmConfigFile, ChannelRuntimeStatus, DataFile, DetectionHistoryFile,
    DetectionSampleRecord, HistoryFile, NotificationConfigFile, NotificationHistoryFile,
    NotificationRecord, RoiConfigFile, SceneState, SecurityConfig, SourceGroup, StateRecord,
    VideoSource,
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

#[derive(Debug, Clone, Copy)]
pub struct PlaybackPosition {
    pub position_sec: f64,
    pub playing: bool,
    pub updated_at: i64,
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
    pub playback_positions: Mutex<HashMap<String, PlaybackPosition>>,
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
            playback_positions: Mutex::new(HashMap::new()),
        }))
    }

    pub fn update_playback_position(&self, source_id: String, position_sec: f64, playing: bool) {
        self.playback_positions.lock().insert(
            source_id,
            PlaybackPosition {
                position_sec,
                playing,
                updated_at: chrono::Utc::now().timestamp_millis(),
            },
        );
    }

    fn current_playback_position(&self, source_id: &str, now: i64) -> Option<f64> {
        let position = self.playback_positions.lock().get(source_id).copied()?;
        extrapolate_playback_position(position, now)
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

    fn push_detection_sample(&self, rec: DetectionSampleRecord) {
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
    }

    /// 命令入口使用：追加后立即持久化。
    pub fn record_detection_sample(&self, rec: DetectionSampleRecord) -> anyhow::Result<()> {
        self.push_detection_sample(rec);
        self.persist_detection_history()
    }

    /// 高频检测循环使用：只追加到内存，由循环定时批量持久化。
    fn buffer_detection_sample(&self, rec: DetectionSampleRecord) {
        self.push_detection_sample(rec);
    }

    fn persist_detection_history(&self) -> anyhow::Result<()> {
        let snapshot = self.detection_history.lock().clone();
        save_json_compact(&self.detection_history_file, &snapshot)
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

fn extrapolate_playback_position(position: PlaybackPosition, now: i64) -> Option<f64> {
    let age_ms = now.saturating_sub(position.updated_at);
    if age_ms > 15_000 {
        return None;
    }
    Some(
        position.position_sec
            + if position.playing {
                age_ms as f64 / 1000.0
            } else {
                0.0
            },
    )
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
        let mut yolo_logged_sources: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut vlm_hourly_usage: HashMap<String, (i64, u32)> = HashMap::new();
        let mut pending_vlm_jobs: HashMap<
            String,
            (
                i64,
                tokio::task::JoinHandle<anyhow::Result<vlm::VlmDetection>>,
            ),
        > = HashMap::new();
        let mut media_durations: HashMap<String, Option<f64>> = HashMap::new();
        let mut last_detection_history_flush = 0_i64;
        let mut detection_history_dirty = false;
        // YOLO 检测：每源一个 WebSocket 客户端 + 熔断器
        // WS 长连接复用，避免每帧建连/拆连的开销
        let mut yolo_clients: HashMap<String, Arc<yolo_detector::YoloClient>> = HashMap::new();
        let mut yolo_breakers: HashMap<String, yolo_detector::YoloCircuitBreaker> = HashMap::new();
        let ffmpeg_concurrency = std::thread::available_parallelism()
            .map(|count| (count.get() / 4).clamp(1, 2))
            .unwrap_or(1);
        let ffmpeg_slots = Arc::new(tokio::sync::Semaphore::new(ffmpeg_concurrency));
        let vlm_ffmpeg_slots = Arc::new(tokio::sync::Semaphore::new(1));
        log::info!(
            "[抽帧] 常规 ffmpeg 最大并发数={ffmpeg_concurrency}，VLM 原图抽帧并发数=1，每进程线程数=1"
        );
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
                seek_secs: Option<f64>,
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
                    // 检测每 N 秒运行一次，本地视频也必须前进 N 秒。旧逻辑每轮只
                    // 前进 1 秒，运行 5 分钟后检测帧会比播放器画面落后数分钟。
                    let estimated = current_counter.saturating_sub(1) as f64
                        * decision.effective_config.simple_interval_sec.max(1) as f64;
                    let base = state
                        .current_playback_position(&s.id, now)
                        .unwrap_or(estimated);
                    Some(match duration {
                        Some(duration) if duration > 0.5 => base % duration,
                        _ => base,
                    })
                } else {
                    None
                };
                // 常规检测仅在 YOLO 模式使用 1280x720。VLM 原始分辨率帧在真正
                // 触发确认时通过独立、单并发的 ffmpeg 路径按需抽取。
                let yolo_enabled = decision.effective_config.yolo_enabled;
                let (frame_width, frame_height) = if yolo_enabled {
                    (1280, 720)
                } else {
                    (320, 240)
                };
                let permit = Arc::clone(&ffmpeg_slots)
                    .acquire_owned()
                    .await
                    .expect("ffmpeg semaphore is open");
                let join_handle = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
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
                        log::warn!(
                            "[抽帧] {} {}x{} 耗时 {}ms (seek={:?})",
                            url.chars().take(40).collect::<String>(),
                            frame_width,
                            frame_height,
                            ms,
                            seek_secs
                        );
                    }
                    result
                });
                pending.push(PendingExtraction {
                    source_idx: idx,
                    source_id: s.id.clone(),
                    source_enabled: s.enabled,
                    effective_config: decision.effective_config.clone(),
                    seek_secs,
                    join_handle,
                });
            }

            // ── Phase 2: 统一等待所有抽帧完成（并发，总延迟 ≈ max 单路） ──────
            struct ExtractionResult {
                source_idx: usize,
                effective_config: AlgorithmConfig,
                seek_secs: Option<f64>,
                frame: crate::pipeline::decoder::DecodedFrame,
            }
            let mut results: Vec<ExtractionResult> = Vec::new();
            for p in pending {
                match p.join_handle.await {
                    Ok(Ok(frame)) => {
                        results.push(ExtractionResult {
                            source_idx: p.source_idx,
                            effective_config: p.effective_config,
                            seek_secs: p.seek_secs,
                            frame,
                        });
                    }
                    Ok(Err(err)) => {
                        let msg = format!("真实帧抽取失败: {err}");
                        {
                            let mut runtime = state.runtime_status.lock();
                            let entry = runtime.entry(p.source_id.clone()).or_insert_with(|| {
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
                            let entry = runtime.entry(p.source_id.clone()).or_insert_with(|| {
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

            // YOLO 网络调用按视频源并发执行。旧实现虽然并发抽帧，但随后逐路
            // 等待最多 7 秒，8 路视频会把 10 秒周期拖成 20 秒甚至更长。
            let mut yolo_results: HashMap<String, Result<yolo_detector::YoloDetectResult, String>> =
                HashMap::new();
            let mut yolo_jobs = Vec::new();
            for r in &results {
                let s = &sources[r.source_idx];
                let cfg = &r.effective_config;
                if !cfg.yolo_enabled {
                    continue;
                }
                if yolo_logged_sources.insert(s.id.clone()) {
                    log::info!(
                        "[yolo] 通道 {} 首次进入 YOLO 检测分支 (api={}, conf={})",
                        s.id,
                        yolo_detector::redact_url(&cfg.yolo_api_base),
                        cfg.yolo_confidence
                    );
                }
                let breaker = yolo_breakers.entry(s.id.clone()).or_default();
                if breaker.is_open(now) {
                    yolo_results.insert(s.id.clone(), Err("YOLO 服务熔断冷却中".into()));
                    continue;
                }
                let client = yolo_clients.entry(s.id.clone()).or_insert_with(|| {
                    Arc::new(yolo_detector::YoloClient::new(cfg.yolo_api_base.clone()))
                });
                if client.api_base() != cfg.yolo_api_base {
                    *client = Arc::new(yolo_detector::YoloClient::new(cfg.yolo_api_base.clone()));
                }
                let jpeg = match yolo_detector::encode_frame_as_jpeg(&r.frame) {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        breaker.record_failure(now);
                        yolo_results.insert(s.id.clone(), Err(format!("YOLO 帧编码失败: {err}")));
                        continue;
                    }
                };
                let source_id = s.id.clone();
                let confidence = cfg.yolo_confidence;
                let client = Arc::clone(client);
                yolo_jobs.push(async move {
                    let result = match tokio::time::timeout(
                        Duration::from_secs(7),
                        client.detect_frame(jpeg, confidence),
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(_) => {
                            client.disconnect().await;
                            Err("YOLO 检测超时（>7s）".into())
                        }
                    };
                    (source_id, result)
                });
            }
            for (source_id, result) in futures_util::future::join_all(yolo_jobs).await {
                let breaker = yolo_breakers.entry(source_id.clone()).or_default();
                if result.is_ok() {
                    breaker.record_success();
                } else {
                    breaker.record_failure(now);
                }
                yolo_results.insert(source_id, result);
            }

            // ── Phase 3: 顺序处理检测结果（VLM / 报警 / 事件推送） ──────────
            for r in results {
                let s = &sources[r.source_idx];
                let decision_effective_config = r.effective_config;
                let frame_seek_secs = r.seek_secs;
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
                    match yolo_results.remove(&s.id) {
                        Some(Ok(result)) => {
                            let cnt = yolo_success_count.entry(s.id.clone()).or_insert(0);
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
                            new_state.model_latency_ms = Some(result.process_ms as u32);
                            yolo_detections = result.detections;
                        }
                        Some(Err(err)) => {
                            log::error!("[yolo] {} 检测失败: {}", s.id, err);
                            new_state.reason = Some(format!("yolo_error: {err}"));
                            yolo_error_msg = Some(err);
                        }
                        None => {
                            new_state.reason = Some("yolo_error: 结果缺失".into());
                            yolo_error_msg = Some("YOLO 检测结果缺失".into());
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
                let yolo_failure_active =
                    decision_effective_config.yolo_enabled && yolo_error_msg.is_some();
                if yolo_failure_active && previous_yolo_error.is_none() {
                    let app_for_notify = app.clone();
                    let state_for_notify = state.inner().clone();
                    let source_id = s.id.clone();
                    let detail = yolo_error_msg
                        .clone()
                        .unwrap_or_else(|| "YOLO 服务器失效".into());
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
                let tracker = presence_trackers.entry(s.id.clone()).or_default();
                let simple_person = new_state.person;
                let simple_person_confidence = new_state.person_confidence;
                let mut vlm_person: Option<bool> = None;
                let mut vlm_person_confidence: Option<f32> = None;
                let mut vlm_detections: Option<Vec<vlm::VlmDetectionBox>> = None;
                let mut vlm_frame_width: Option<u32> = None;
                let mut vlm_frame_height: Option<u32> = None;
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
                let mut alarm_transition = alarm_timers.entry(s.id.clone()).or_default().update(
                    raw_alarm,
                    recover_condition,
                    now,
                    decision_effective_config.alarm_hold_sec,
                    decision_effective_config.alarm_recover_sec,
                );
                let mut alarm_countdown_progress = alarm_timers
                    .get(&s.id)
                    .map(|timer| {
                        timer.alarm_progress(now, decision_effective_config.alarm_hold_sec)
                    })
                    .unwrap_or(0.0);
                let mut alarm_status = alarm_timers
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
                let mut vlm_error_msg: Option<String> = None;

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

                let mut alarm_cancelled_by_vlm = false;
                let mut vlm_confirmation_completed = false;

                // 异常条件已消失时，不再等待旧画面的 VLM 结果。
                if !raw_alarm {
                    if let Some((_started, job)) = pending_vlm_jobs.remove(&s.id) {
                        job.abort();
                        if let Some(timer) = alarm_timers.get_mut(&s.id) {
                            timer.reset();
                        }
                        alarm_transition = None;
                        alarm_status = "normal";
                        alarm_countdown_progress = 0.0;
                    }
                }

                let vlm_job_finished = pending_vlm_jobs
                    .get(&s.id)
                    .map(|(_, job)| job.is_finished())
                    .unwrap_or(false);
                if vlm_job_finished {
                    let (started, job) = pending_vlm_jobs.remove(&s.id).expect("VLM job exists");
                    let latency = chrono::Utc::now()
                        .timestamp_millis()
                        .saturating_sub(started);
                    vlm_confirmation_completed = true;
                    match job.await {
                        Ok(Ok(vlm_result)) => {
                            usage_entry.1 = usage_entry.1.saturating_add(1);
                            vlm_person = Some(vlm_result.has_person);
                            vlm_person_confidence = Some(vlm_result.confidence);
                            vlm_detections = Some(vlm_result.detections.clone());
                            vlm_frame_width = Some(vlm_result.image_width);
                            vlm_frame_height = Some(vlm_result.image_height);
                            new_state.model_latency_ms = Some(latency as u32);
                            if vlm_result.has_person {
                                vlm_status = "person_alarm_cancelled";
                                if let Some(timer) = alarm_timers.get_mut(&s.id) {
                                    timer.reset();
                                }
                                alarm_transition = None;
                                alarm_status = "normal";
                                alarm_countdown_progress = 0.0;
                                alarm_cancelled_by_vlm = true;
                                log::info!(
                                    "[vlm] 报警确认：检测到人员，取消报警 source={} confidence={:.2} latency={}ms",
                                    s.id, vlm_result.confidence, latency
                                );
                            } else {
                                vlm_status = "no_person_alarm_confirmed";
                                alarm_transition = Some(true);
                                alarm_status = "alarm_active";
                                log::info!(
                                    "[vlm] 报警确认：无人，继续报警 source={} confidence={:.2} latency={}ms",
                                    s.id, vlm_result.confidence, latency
                                );
                            }
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
                        Ok(Err(err)) => {
                            vlm_status = "error";
                            vlm_detections = Some(Vec::new());
                            vlm_error_msg = Some(format!("VLM 检测失败: {err}"));
                            alarm_transition = Some(true);
                            alarm_status = "alarm_active";
                            log::warn!(
                                "[vlm] 报警确认失败，继续报警 source={} error={}",
                                s.id,
                                err
                            );
                        }
                        Err(err) => {
                            vlm_status = "error";
                            vlm_detections = Some(Vec::new());
                            vlm_error_msg = Some(format!("VLM 检测任务异常: {err}"));
                            alarm_transition = Some(true);
                            alarm_status = "alarm_active";
                        }
                    }
                } else if pending_vlm_jobs.contains_key(&s.id) {
                    alarm_transition = None;
                    alarm_status = "vlm_checking";
                    vlm_status = "checking";
                }

                // 首次进入正式报警时只启动后台确认，不阻塞其他视频源和下一轮检测。
                if alarm_transition == Some(true)
                    && !vlm_confirmation_completed
                    && decision_effective_config.vlm_enabled
                    && vlm_under_limit
                    && !light_off_skip
                {
                    let config = decision_effective_config.clone();
                    let source_url = s.url.clone();
                    let vlm_slots = Arc::clone(&vlm_ffmpeg_slots);
                    let started = chrono::Utc::now().timestamp_millis();
                    let job = tokio::spawn(async move {
                        let permit = vlm_slots
                            .acquire_owned()
                            .await
                            .map_err(|_| anyhow::anyhow!("VLM ffmpeg 调度器已关闭"))?;
                        let vlm_frame = tokio::task::spawn_blocking(move || {
                            extract_original_frame_from_url_at(
                                &source_url,
                                Duration::from_secs(10),
                                frame_seek_secs,
                            )
                        })
                        .await
                        .map_err(|err| anyhow::anyhow!("VLM 原图抽帧任务异常: {err}"))??;
                        drop(permit);
                        vlm::analyze_person(&config, &vlm_frame).await
                    });
                    pending_vlm_jobs.insert(s.id.clone(), (started, job));
                    alarm_transition = None;
                    alarm_status = "vlm_checking";
                    vlm_status = "checking";
                }

                if let Some(alarm_active) = alarm_transition {
                    if !alarm_cancelled_by_vlm {
                        // 通知任务会读取 current_state；先写入本轮最终检测结果，
                        // 避免异步通知拿到上一帧状态。
                        state
                            .current_state
                            .lock()
                            .insert(s.id.clone(), new_state.clone());
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

                // VLM 决策完成后再一次性提交最终状态，避免前端收到半成品状态。
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
                    entry.vlm_enabled = decision_effective_config.vlm_enabled;
                    entry.yolo_enabled = decision_effective_config.yolo_enabled;
                    entry.yolo_error = yolo_error_msg.clone();
                    entry.last_error = yolo_error_msg.clone().or_else(|| vlm_error_msg.clone());
                    entry.last_algorithm_at = Some(now);
                    entry.alarm_status = alarm_status.into();
                    entry.ts = now;
                }
                let scene_payload = serde_json::json!({
                    "source_id": s.id,
                    "person": person,
                    "light": light,
                    "light_state": if light { "on" } else { "off" },
                    "alarm": is_formal_alarm_status(alarm_status),
                    "alarm_record_active": alarm_timers.get(&s.id).map(|timer| timer.active).unwrap_or(false),
                    "alarm_status": alarm_status,
                    "alarm_progress": alarm_countdown_progress,
                    "alarm_countdown_progress": alarm_countdown_progress,
                    "simple_person": simple_person,
                    "simple_person_confidence": simple_person_confidence,
                    "vlm_person": vlm_person,
                    "vlm_person_confidence": vlm_person_confidence,
                    "vlm_detections": vlm_detections,
                    "vlm_frame_width": vlm_frame_width,
                    "vlm_frame_height": vlm_frame_height,
                    "vlm_status": vlm_status,
                    "yolo_error": yolo_error_msg,
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
                if decision_effective_config.developer_mode {
                    state.buffer_detection_sample(DetectionSampleRecord::from_scene(
                        &s.id,
                        &new_state,
                        alarm_status,
                        now,
                    ));
                    detection_history_dirty = true;
                }
            }
            if detection_history_dirty && now.saturating_sub(last_detection_history_flush) >= 30_000
            {
                if let Err(e) = state.persist_detection_history() {
                    log::warn!("详细检测历史批量落库失败: {e}");
                } else {
                    last_detection_history_flush = now;
                    detection_history_dirty = false;
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
            .map(|status| is_formal_alarm_status(status.alarm_status.as_str()))
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

fn is_formal_alarm_status(status: &str) -> bool {
    matches!(status, "alarm_active" | "acknowledged" | "recovering")
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

    fn person_is_held(&self, now: i64) -> bool {
        self.last_person_seen_at
            .map(|seen_at| now.saturating_sub(seen_at) < PERSON_PRESENCE_HOLD_MS)
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
    use super::{
        extrapolate_playback_position, is_formal_alarm_status, migrate_algorithm_default_tuning,
        should_recover_alarm, AlarmTimer, PlaybackPosition,
    };
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
    fn formal_alarm_status_matches_notification_lifecycle() {
        assert!(is_formal_alarm_status("alarm_active"));
        assert!(is_formal_alarm_status("acknowledged"));
        assert!(is_formal_alarm_status("recovering"));
        assert!(!is_formal_alarm_status("suspected"));
        assert!(!is_formal_alarm_status("vlm_checking"));
        assert!(!is_formal_alarm_status("resolved"));
    }

    #[test]
    fn playback_position_tracks_playing_paused_and_stale_video() {
        let playing = PlaybackPosition {
            position_sec: 42.0,
            playing: true,
            updated_at: 1_000,
        };
        assert_eq!(extrapolate_playback_position(playing, 3_500), Some(44.5));

        let paused = PlaybackPosition {
            playing: false,
            ..playing
        };
        assert_eq!(extrapolate_playback_position(paused, 3_500), Some(42.0));
        assert_eq!(extrapolate_playback_position(playing, 17_001), None);
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
