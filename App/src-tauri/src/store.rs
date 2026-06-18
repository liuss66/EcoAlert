// 视频源数据模型 + 持久化 + 分组 + 状态历史
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::Path;
use uuid::Uuid;

pub const CONFIG_SCHEMA_VERSION: u32 = 1;

/// 视频源分组
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceGroup {
    pub id: String,
    pub name: String,
    /// 排序权重（拖拽后调整）
    pub order: i32,
    /// 是否折叠
    pub collapsed: bool,
    /// 域检测：开启后，分组内所有启用视频源均报警时才发送外部报警通知
    #[serde(default)]
    pub domain_detection_enabled: bool,
    pub created_at: i64,
}

impl SourceGroup {
    pub fn new(name: impl Into<String>, order: i32) -> Self {
        Self {
            id: format!("grp-{}", Uuid::new_v4().simple()),
            name: name.into(),
            order,
            collapsed: false,
            domain_detection_enabled: false,
            created_at: chrono::Utc::now().timestamp_millis(),
        }
    }
}

/// 视频源（带 group_id）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoSource {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(rename = "type")]
    pub source_type: String, // hls | mp4 | webcam | rtsp
    pub location: String,
    pub enabled: bool,
    /// 所属分组 id（None = 默认"未分组"）
    #[serde(default)]
    pub group_id: Option<String>,
    /// 在所属组内的排序权重
    #[serde(default)]
    pub order: i32,
    pub created_at: i64,
}

impl VideoSource {
    pub fn new(
        name: String,
        url: String,
        source_type: String,
        location: String,
        enabled: bool,
        group_id: Option<String>,
        order: i32,
    ) -> Self {
        Self {
            id: format!("src-{}", Uuid::new_v4().simple()),
            name,
            url,
            source_type,
            location,
            enabled,
            group_id,
            order,
            created_at: chrono::Utc::now().timestamp_millis(),
        }
    }
}

/// 算法输出的单帧场景状态
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SceneState {
    /// 是否有人
    pub person: bool,
    /// 是否开灯
    pub light: bool,
    /// 帧序号（便于排错）
    #[serde(default)]
    pub frame_seq: u64,
    /// 算法给出的置信度（0..1），仅作参考
    #[serde(default)]
    pub confidence: f32,
    /// 检测结果来源：simple | vlm | fused | mock
    #[serde(default)]
    pub source: String,
    /// 人员检测置信度（0..1）
    #[serde(default)]
    pub person_confidence: f32,
    /// 灯光检测置信度（0..1）
    #[serde(default)]
    pub light_confidence: f32,
    /// 判定原因
    #[serde(default)]
    pub reason: Option<String>,
    /// 模型推理耗时（毫秒）
    #[serde(default)]
    pub model_latency_ms: Option<u32>,
    /// 灯光 ROI 或全帧平均亮度（0..255）
    #[serde(default)]
    pub light_brightness: f32,
    /// 彩色程度（0..1）。彩色摄像头开灯时通常较高，红外黑白时接近 0。
    #[serde(default)]
    pub color_score: f32,
    /// 帧差运动面积比例（0..1），当前人员检测的临时代理信号
    #[serde(default)]
    pub motion_score: f32,
    /// 本次检测总耗时（毫秒）
    #[serde(default)]
    pub process_ms: f32,
}

/// 单条状态历史记录（只在状态变化时落库）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateRecord {
    pub id: String,
    pub source_id: String,
    pub person: bool,
    pub light: bool,
    /// 派生：是否报警（无人 + 亮灯）
    pub alarm: bool,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRuntimeStatus {
    pub source_id: String,
    pub online_status: String,
    pub algorithm_status: String,
    pub alarm_status: String,
    #[serde(default)]
    pub vlm_enabled: bool,
    #[serde(default)]
    pub yolo_enabled: bool,
    #[serde(default)]
    pub yolo_error: Option<String>,
    pub last_frame_at: Option<i64>,
    pub last_algorithm_at: Option<i64>,
    pub last_error: Option<String>,
    pub effective_algorithm_config_scope: String,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmRecord {
    pub id: String,
    pub source_id: String,
    pub status: String,
    pub first_seen_at: i64,
    pub triggered_at: Option<i64>,
    pub acknowledged_at: Option<i64>,
    pub resolved_at: Option<i64>,
    pub acknowledged_by: Option<String>,
    pub note: Option<String>,
    pub last_state_id: Option<String>,
}

impl AlarmRecord {
    pub fn new_active(source_id: String, ts: i64, last_state_id: Option<String>) -> Self {
        Self {
            id: format!("alm-{}", Uuid::new_v4().simple()),
            source_id,
            status: "alarm_active".into(),
            first_seen_at: ts,
            triggered_at: Some(ts),
            acknowledged_at: None,
            resolved_at: None,
            acknowledged_by: None,
            note: None,
            last_state_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmRecordFile {
    pub schema_version: u32,
    #[serde(default, with = "vecdeque_serde")]
    pub records: VecDeque<AlarmRecord>,
}

impl Default for AlarmRecordFile {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            records: VecDeque::new(),
        }
    }
}

impl ChannelRuntimeStatus {
    pub fn new(source_id: String, enabled: bool, ts: i64) -> Self {
        Self {
            source_id,
            online_status: if enabled { "online" } else { "offline" }.into(),
            algorithm_status: if enabled { "idle" } else { "disabled" }.into(),
            alarm_status: "normal".into(),
            vlm_enabled: false,
            yolo_enabled: false,
            yolo_error: None,
            last_frame_at: enabled.then_some(ts),
            last_algorithm_at: None,
            last_error: None,
            effective_algorithm_config_scope: "global".into(),
            ts,
        }
    }
}

impl StateRecord {
    pub fn from_change(source_id: &str, state: &SceneState) -> Self {
        let alarm = !state.person && state.light;
        Self {
            id: format!("rec-{}", Uuid::new_v4().simple()),
            source_id: source_id.to_string(),
            person: state.person,
            light: state.light,
            alarm,
            ts: chrono::Utc::now().timestamp_millis(),
        }
    }
}

/// 全部数据文件
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DataFile {
    pub sources: Vec<VideoSource>,
    #[serde(default)]
    pub groups: Vec<SourceGroup>,
}

/// 状态历史（独立文件，避免 sources.json 被刷大）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryFile {
    /// 每个源最近 N 条变更，按时间倒序
    #[serde(with = "vecdeque_serde")]
    pub records: VecDeque<StateRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionSampleRecord {
    pub source_id: String,
    pub ts: i64,
    pub frame_seq: u64,
    pub person: bool,
    pub light: bool,
    pub alarm: bool,
    pub alarm_status: String,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub person_confidence: f32,
    #[serde(default)]
    pub light_confidence: f32,
    #[serde(default)]
    pub light_brightness: f32,
    #[serde(default)]
    pub color_score: f32,
    #[serde(default)]
    pub motion_score: f32,
    #[serde(default)]
    pub process_ms: f32,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub reason: Option<String>,
}

impl DetectionSampleRecord {
    pub fn from_scene(source_id: &str, state: &SceneState, alarm_status: &str, ts: i64) -> Self {
        Self {
            source_id: source_id.to_string(),
            ts,
            frame_seq: state.frame_seq,
            person: state.person,
            light: state.light,
            alarm: alarm_status == "alarm_active",
            alarm_status: alarm_status.to_string(),
            confidence: state.confidence,
            person_confidence: state.person_confidence,
            light_confidence: state.light_confidence,
            light_brightness: state.light_brightness,
            color_score: state.color_score,
            motion_score: state.motion_score,
            process_ms: state.process_ms,
            source: state.source.clone(),
            reason: state.reason.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DetectionHistoryFile {
    pub schema_version: u32,
    #[serde(default, with = "vecdeque_serde")]
    pub records: VecDeque<DetectionSampleRecord>,
}

pub fn load(path: &Path) -> DataFile {
    if !path.exists() {
        return DataFile::default();
    }
    match fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => DataFile::default(),
    }
}

pub fn save(path: &Path, data: &DataFile) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let s = serde_json::to_string_pretty(data)?;
    write_temp_then_replace(path, s)?;
    Ok(())
}

pub fn load_history(path: &Path) -> HistoryFile {
    if !path.exists() {
        return HistoryFile::default();
    }
    match fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => HistoryFile::default(),
    }
}

pub fn save_history(path: &Path, data: &HistoryFile) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let s = serde_json::to_string_pretty(data)?;
    write_temp_then_replace(path, s)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveWindow {
    #[serde(default)]
    pub weekdays: Vec<u8>,
    pub start: String,
    pub end: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
}

fn default_timezone() -> String {
    "Local".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlgorithmConfig {
    pub enabled: bool,
    #[serde(default)]
    pub developer_mode: bool,
    pub scope: String,
    #[serde(default)]
    pub scope_id: Option<String>,
    #[serde(default)]
    pub active_windows: Vec<ActiveWindow>,
    #[serde(default)]
    pub exception_windows: Vec<ActiveWindow>,
    pub simple_interval_sec: u32,
    pub vlm_interval_sec: u32,
    pub vlm_enabled: bool,
    pub vlm_skip_when_person: bool,
    #[serde(default)]
    pub vlm_api_base: String,
    #[serde(default)]
    pub vlm_api_key: String,
    #[serde(default)]
    pub vlm_model: String,
    #[serde(default = "default_vlm_prompt")]
    pub vlm_prompt: String,
    #[serde(default = "default_vlm_temperature")]
    pub vlm_temperature: f32,
    #[serde(default = "default_vlm_max_tokens")]
    pub vlm_max_tokens: u32,
    #[serde(default)]
    pub vlm_cost_enabled: bool,
    #[serde(default)]
    pub vlm_price_input: f32,
    #[serde(default)]
    pub vlm_price_input_cache: f32,
    #[serde(default)]
    pub vlm_price_output: f32,
    #[serde(default)]
    pub vlm_price_output_cache: f32,
    pub person_threshold: f32,
    pub light_threshold: f32,
    pub alarm_hold_sec: u32,
    pub alarm_recover_sec: u32,
    pub recover_policy: String,
    pub vlm_hourly_limit: u32,
    #[serde(default)]
    pub roi_version: Option<String>,
    // ── YOLO 目标检测 ─────────────────────────────────────────────────────
    #[serde(default)]
    pub yolo_enabled: bool,
    #[serde(default = "default_yolo_api_base")]
    pub yolo_api_base: String,
    #[serde(default = "default_yolo_confidence")]
    pub yolo_confidence: f32,
}

impl Default for AlgorithmConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            developer_mode: false,
            scope: "global".into(),
            scope_id: None,
            // 空启用窗口表示全天运行。演示 / release 默认需要能立即抽帧检测。
            active_windows: vec![],
            exception_windows: vec![],
            simple_interval_sec: 1,
            vlm_interval_sec: 300,
            vlm_enabled: false,
            vlm_skip_when_person: true,
            vlm_api_base: String::new(),
            vlm_api_key: String::new(),
            vlm_model: String::new(),
            vlm_prompt: default_vlm_prompt(),
            vlm_temperature: default_vlm_temperature(),
            vlm_max_tokens: default_vlm_max_tokens(),
            vlm_cost_enabled: false,
            vlm_price_input: 0.0,
            vlm_price_input_cache: 0.0,
            vlm_price_output: 0.0,
            vlm_price_output_cache: 0.0,
            person_threshold: 0.003,
            light_threshold: 0.70,
            alarm_hold_sec: 300,
            alarm_recover_sec: 60,
            recover_policy: "either".into(),
            vlm_hourly_limit: 12,
            roi_version: None,
            yolo_enabled: false,
            yolo_api_base: default_yolo_api_base(),
            yolo_confidence: default_yolo_confidence(),
        }
    }
}

fn default_yolo_api_base() -> String {
    "ws://localhost:8090".into()
}

fn default_yolo_confidence() -> f32 {
    0.45
}

fn default_vlm_temperature() -> f32 {
    0.1
}

fn default_vlm_max_tokens() -> u32 {
    2048
}

pub fn default_vlm_prompt() -> String {
    r#"你是一个专业的人体目标检测系统。请仔细分析这张图片，检测其中是否包含人体（完整或局部均可，包括背影、侧身、被部分遮挡的人）。

你必须严格按照以下 JSON 格式输出，不要包含任何额外文字、解释或说明：

当检测到人时：
{"has_person": true, "detections": [{"label": "person", "confidence": 0.95, "bbox": [x1, y1, x2, y2]}]}

当未检测到人时：
{"has_person": false, "detections": []}

要求：
1. bbox 坐标采用千分制归一化值（范围 0-1000），[x1, y1] 为边界框左上角，[x2, y2] 为右下角
2. confidence 为 0-1 之间的浮点数，表示检测置信度
3. 检测到的每一个人都必须单独列出一条记录
4. 仅输出 JSON，不要包含 markdown 标记、代码块或其他任何文字"#.into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlgorithmConfigFile {
    pub schema_version: u32,
    #[serde(default)]
    pub manual_source_config_mode: bool,
    pub global: AlgorithmConfig,
    #[serde(default)]
    pub groups: HashMap<String, AlgorithmConfig>,
    #[serde(default)]
    pub sources: HashMap<String, AlgorithmConfig>,
}

impl Default for AlgorithmConfigFile {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            manual_source_config_mode: true,
            global: AlgorithmConfig::default(),
            groups: HashMap::new(),
            sources: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RoiRect {
    pub id: String,
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoiConfig {
    pub source_id: String,
    pub version: String,
    #[serde(default)]
    pub light_rois: Vec<RoiRect>,
    #[serde(default)]
    pub exclude_rois: Vec<RoiRect>,
    #[serde(default)]
    pub person_rois: Vec<RoiRect>,
    #[serde(default = "default_light_threshold")]
    pub light_threshold: f32,
    /// 旧版开灯阈值，仅用于反序列化兼容；不再写出。
    #[serde(default, skip_serializing, rename = "lightOnThreshold")]
    _legacy_light_on_threshold: Option<f32>,
    /// 旧版关灯阈值，仅用于反序列化兼容；不再写出。
    #[serde(default, skip_serializing, rename = "lightOffThreshold")]
    _legacy_light_off_threshold: Option<f32>,
    pub updated_at: i64,
}

pub const GLOBAL_ROI_SOURCE_ID: &str = "__global__";

fn default_light_threshold() -> f32 {
    0.015
}

impl RoiConfig {
    pub fn new(source_id: String) -> Self {
        Self {
            source_id,
            version: format!("roi-{}", Uuid::new_v4().simple()),
            light_rois: vec![],
            exclude_rois: vec![],
            person_rois: vec![],
            light_threshold: 0.015,
            _legacy_light_on_threshold: None,
            _legacy_light_off_threshold: None,
            updated_at: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// 加载旧配置时，若 `light_threshold` 仍为默认值而旧版字段存在，则用旧值覆盖。
    pub fn apply_legacy_threshold_migration(&mut self) {
        if (self.light_threshold - default_light_threshold()).abs() < f32::EPSILON {
            if let Some(on) = self._legacy_light_on_threshold {
                if on > 0.0 && on <= 0.2 {
                    self.light_threshold = on;
                }
            } else if let Some(off) = self._legacy_light_off_threshold {
                if off > 0.0 && off <= 0.2 {
                    self.light_threshold = off;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoiConfigFile {
    pub schema_version: u32,
    #[serde(default)]
    pub manual_source_config_mode: bool,
    #[serde(default = "default_global_roi_config")]
    pub global: RoiConfig,
    #[serde(default)]
    pub by_source: HashMap<String, RoiConfig>,
}

impl Default for RoiConfigFile {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            manual_source_config_mode: true,
            global: default_global_roi_config(),
            by_source: HashMap::new(),
        }
    }
}

fn default_global_roi_config() -> RoiConfig {
    RoiConfig::new(GLOBAL_ROI_SOURCE_ID.into())
}

impl RoiConfigFile {
    /// 将旧版 `lightOnThreshold / lightOffThreshold` 迁移到 `light_threshold`。
    pub fn migrate_legacy_thresholds(&mut self) {
        self.global.apply_legacy_threshold_migration();
        for cfg in self.by_source.values_mut() {
            cfg.apply_legacy_threshold_migration();
        }
    }

    pub fn effective_for_source(&self, source_id: &str) -> RoiConfig {
        self.by_source.get(source_id).cloned().unwrap_or_else(|| {
            let mut global = self.global.clone();
            global.source_id = source_id.to_string();
            global
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HeaderPair {
    pub name: String,
    pub value: String,
}

fn default_channel_type() -> String {
    "webhook".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationTarget {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    #[serde(default = "default_channel_type")]
    pub channel_type: String,
    pub url: String,
    pub method: String,
    #[serde(default)]
    pub headers: Vec<HeaderPair>,
    pub body_template: String,
    pub timeout_sec: u32,
    pub retry_count: u32,
    #[serde(default)]
    pub event_types: Vec<String>,
    pub cooldown_sec: u32,
    pub created_at: i64,

    // ---- API 凭证模式（webhook 模式不填） ----
    /// 飞书 App ID / 企微 CorpID / QQ AppID
    #[serde(default)]
    pub app_id: String,
    /// 飞书 App Secret / 企微 Secret / QQ ClientSecret
    #[serde(default)]
    pub app_secret: String,
    /// 企微 AgentID（仅企微需要）
    #[serde(default)]
    pub agent_id: String,
    /// 绑定目标：飞书 chat_id / 企微 touser / QQ group_openid
    #[serde(default)]
    pub chat_id: String,

    // ---- Token 缓存（内部使用） ----
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub token_expires_at: i64,
}

impl NotificationTarget {
    pub fn new(mut payload: NotificationTargetPayload) -> Self {
        if payload.method.trim().is_empty() {
            payload.method = "POST".into();
        }
        if payload.channel_type.trim().is_empty() {
            payload.channel_type = "webhook".into();
        }
        Self {
            id: format!("ntf-{}", Uuid::new_v4().simple()),
            name: payload.name,
            enabled: payload.enabled,
            channel_type: payload.channel_type,
            url: payload.url,
            method: payload.method,
            headers: payload.headers,
            body_template: payload.body_template,
            timeout_sec: payload.timeout_sec.unwrap_or(10),
            retry_count: payload.retry_count.unwrap_or(2),
            event_types: payload.event_types,
            cooldown_sec: payload.cooldown_sec.unwrap_or(1800),
            created_at: chrono::Utc::now().timestamp_millis(),
            app_id: payload.app_id,
            app_secret: payload.app_secret,
            agent_id: payload.agent_id,
            chat_id: payload.chat_id,
            access_token: String::new(),
            token_expires_at: 0,
        }
    }

    /// 是否为 API 凭证模式（非 Webhook）
    #[allow(dead_code)]
    pub fn is_api_mode(&self) -> bool {
        self.channel_type != "webhook" && !self.app_id.is_empty()
    }

    /// Token 是否即将过期（60 秒内）
    pub fn token_needs_refresh(&self) -> bool {
        if self.access_token.is_empty() {
            return true;
        }
        let now = chrono::Utc::now().timestamp();
        self.token_expires_at - now < 60
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationRecord {
    pub id: String,
    pub target_id: String,
    pub target_name: String,
    pub event: String,
    pub source_id: Option<String>,
    pub alarm_id: Option<String>,
    pub ok: bool,
    pub status_code: Option<u16>,
    pub error: Option<String>,
    pub request_at: i64,
    pub latency_ms: Option<u32>,
    pub retry_count: u32,
    pub request_body: Option<String>,
}

impl NotificationRecord {
    pub fn new_pending(
        target: &NotificationTarget,
        event: String,
        source_id: Option<String>,
        alarm_id: Option<String>,
        request_body: Option<String>,
        request_at: i64,
    ) -> Self {
        Self {
            id: format!("nhr-{}", Uuid::new_v4().simple()),
            target_id: target.id.clone(),
            target_name: target.name.clone(),
            event,
            source_id,
            alarm_id,
            ok: false,
            status_code: None,
            error: None,
            request_at,
            latency_ms: None,
            retry_count: 0,
            request_body,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationHistoryFile {
    pub schema_version: u32,
    #[serde(default, with = "vecdeque_serde")]
    pub records: VecDeque<NotificationRecord>,
}

impl Default for NotificationHistoryFile {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            records: VecDeque::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NotificationTargetPayload {
    pub name: String,
    pub enabled: bool,
    #[serde(default)]
    pub channel_type: String,
    pub url: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub headers: Vec<HeaderPair>,
    #[serde(default)]
    pub body_template: String,
    #[serde(default)]
    pub timeout_sec: Option<u32>,
    #[serde(default)]
    pub retry_count: Option<u32>,
    #[serde(default)]
    pub event_types: Vec<String>,
    #[serde(default)]
    pub cooldown_sec: Option<u32>,
    // API 凭证模式
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub app_secret: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub chat_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationConfigFile {
    pub schema_version: u32,
    #[serde(default)]
    pub targets: Vec<NotificationTarget>,
}

impl Default for NotificationConfigFile {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            targets: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityConfig {
    pub schema_version: u32,
    pub external_vlm_enabled: bool,
    pub save_vlm_snapshots: bool,
    pub snapshot_retention_days: u32,
    pub include_image_in_notification: bool,
    pub blur_person_before_external_send: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            external_vlm_enabled: false,
            save_vlm_snapshots: false,
            snapshot_retention_days: 7,
            include_image_in_notification: false,
            blur_person_before_external_send: true,
        }
    }
}

pub fn load_json<T>(path: &Path) -> T
where
    T: Default + for<'de> Deserialize<'de>,
{
    if !path.exists() {
        return T::default();
    }
    match fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => T::default(),
    }
}

pub fn save_json<T: Serialize>(path: &Path, data: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_temp_then_replace(path, serde_json::to_string_pretty(data)?)?;
    Ok(())
}

/// 给一组 VideoSource 补上默认 group_id（向前兼容旧数据）
pub fn backfill_groups(data: &mut DataFile) {
    let default_grp_id = "grp-default".to_string();
    if !data.groups.iter().any(|g| g.id == default_grp_id) {
        data.groups.insert(
            0,
            SourceGroup {
                id: default_grp_id.clone(),
                name: "默认分组".into(),
                order: 0,
                collapsed: false,
                domain_detection_enabled: false,
                created_at: chrono::Utc::now().timestamp_millis(),
            },
        );
    }
    for s in &mut data.sources {
        if s.group_id.is_none() {
            s.group_id = Some(default_grp_id.clone());
        }
    }
}

/// 调试菜单"测试视频源"开关控制的预设源 ID，关闭开关时精确删除这些源。
pub const TEST_SOURCE_IDS: &[&str] = &[
    "cam-domain-0424",
    "cam-domain-0527",
    "cam-domain-0528",
    "cam-domain-0507",
    "cam-chassis-0424",
    "cam-chassis-0515",
    "cam-chassis-0507",
    "cam-hardware-0514",
];

/// 测试源专用的分组 ID，关闭开关时若分组下已无其他源则一并删除。
pub const TEST_GROUP_IDS: &[&str] = &["grp-domain", "grp-chassis", "grp-hardware"];

/// 写入 8 个本地 HLS 测试源，与 Tools/push_streamer 的 cam-1 ~ cam-8 端点保持一致。
/// 调用方负责决定是否执行（调试菜单开关控制）；已存在的同名分组会跳过，不重复创建。
pub fn seed_local_hls_sources(data: &mut DataFile) {
    let now = chrono::Utc::now().timestamp_millis();
    let group_defs = [
        ("grp-domain", "域控测试视频", 0),
        ("grp-chassis", "底盘测试视频", 1),
        ("grp-hardware", "硬件测试视频", 2),
    ];
    for (id, name, order) in group_defs {
        if !data.groups.iter().any(|group| group.id == id) {
            data.groups.push(SourceGroup {
                id: id.into(),
                name: name.into(),
                order,
                collapsed: false,
                domain_detection_enabled: false,
                created_at: now,
            });
        }
    }

    let source_defs = [
        (
            "cam-domain-0424",
            "4·24 域控",
            1,
            "Video/4·24域控.mp4",
            true,
            "grp-domain",
            0,
        ),
        (
            "cam-domain-0527",
            "5·27 域控",
            5,
            "Video/5·27域控.mp4",
            true,
            "grp-domain",
            1,
        ),
        (
            "cam-domain-0528",
            "5·28 域控",
            6,
            "Video/5·28域控.mp4",
            true,
            "grp-domain",
            2,
        ),
        (
            "cam-domain-0507",
            "5·7 域控",
            7,
            "Video/5·7域控.mp4",
            true,
            "grp-domain",
            3,
        ),
        (
            "cam-chassis-0424",
            "4·24 底盘",
            2,
            "Video/4·24底盘.mp4",
            true,
            "grp-chassis",
            0,
        ),
        (
            "cam-chassis-0515",
            "5·15 底盘",
            4,
            "Video/5·15底盘.mp4",
            true,
            "grp-chassis",
            1,
        ),
        (
            "cam-chassis-0507",
            "5·7 底盘",
            8,
            "Video/5·7底盘.mp4",
            true,
            "grp-chassis",
            2,
        ),
        (
            "cam-hardware-0514",
            "5·14 硬件",
            3,
            "Video/5·14硬件.mp4",
            true,
            "grp-hardware",
            0,
        ),
    ];
    // 追加到已有源列表，跳过 ID 已存在的项（用户可能已手动添加或删除过）
    let existing_ids: std::collections::HashSet<&str> =
        data.sources.iter().map(|s| s.id.as_str()).collect();
    let new_sources: Vec<VideoSource> = source_defs
        .into_iter()
        .filter(|(id, _, _, _, _, _, _)| !existing_ids.contains(id))
        .map(
            |(id, name, cam_no, location, enabled, group_id, order)| VideoSource {
                id: id.into(),
                name: name.into(),
                url: format!("http://127.0.0.1:8080/cam-{cam_no}/index.m3u8"),
                source_type: "hls".into(),
                location: location.into(),
                enabled,
                group_id: Some(group_id.into()),
                order,
                created_at: now,
            },
        )
        .collect();
    data.sources.extend(new_sources);
}

/// 兼容迁移：只修正旧版本内置 HLS 演示源，不改用户手动新增的视频源。
pub fn migrate_local_hls_demo_names(data: &mut DataFile) {
    let group_defs = [
        ("grp-domain", "域控测试视频", 0),
        ("grp-chassis", "底盘测试视频", 1),
        ("grp-hardware", "硬件测试视频", 2),
    ];
    for (id, name, order) in group_defs {
        if let Some(group) = data.groups.iter_mut().find(|group| group.id == id) {
            group.name = name.into();
            group.order = order;
        }
    }

    let mappings = [
        ("cam-a1", "4·24 域控", "Video/4·24域控.mp4", "grp-domain", 0),
        ("cam-b1", "5·27 域控", "Video/5·27域控.mp4", "grp-domain", 1),
        ("cam-b3", "5·28 域控", "Video/5·28域控.mp4", "grp-domain", 2),
        ("cam-c1", "5·7 域控", "Video/5·7域控.mp4", "grp-domain", 3),
        (
            "cam-a2",
            "4·24 底盘",
            "Video/4·24底盘.mp4",
            "grp-chassis",
            0,
        ),
        (
            "cam-a4",
            "5·15 底盘",
            "Video/5·15底盘.mp4",
            "grp-chassis",
            1,
        ),
        ("cam-d1", "5·7 底盘", "Video/5·7底盘.mp4", "grp-chassis", 2),
        (
            "cam-a3",
            "5·14 硬件",
            "Video/5·14硬件.mp4",
            "grp-hardware",
            0,
        ),
    ];
    for (id, name, location, group_id, order) in mappings {
        let found =
            if let Some(source) = data.sources.iter_mut().find(|source| {
                source.id == id && source.url.starts_with("http://127.0.0.1:8080/cam-")
            }) {
                source.name = name.into();
                source.location = location.into();
                source.group_id = Some(group_id.into());
                source.order = order;
                source.enabled = true;
                true
            } else {
                false
            };
        if found && !data.groups.iter().any(|group| group.id == group_id) {
            let (group_name, group_order) = group_defs
                .iter()
                .find(|(gid, _, _)| *gid == group_id)
                .map(|(_, gname, gorder)| (*gname, *gorder))
                .unwrap_or(("测试视频", 999));
            data.groups.push(SourceGroup {
                id: group_id.into(),
                name: group_name.into(),
                order: group_order,
                collapsed: false,
                domain_detection_enabled: false,
                created_at: chrono::Utc::now().timestamp_millis(),
            });
        }
    }

    let old_demo_ids = ["cam-b2", "cam-c2", "cam-c3", "cam-d2"];
    data.sources.retain(|source| {
        !(old_demo_ids.contains(&source.id.as_str())
            && source.url.starts_with("http://127.0.0.1:8080/cam-"))
    });

    let old_demo_group_ids = ["grp-a", "grp-b", "grp-c"];
    data.groups.retain(|group| {
        !old_demo_group_ids.contains(&group.id.as_str())
            || data
                .sources
                .iter()
                .any(|source| source.group_id.as_deref() == Some(group.id.as_str()))
    });
    data.groups.retain(|group| {
        !TEST_GROUP_IDS.contains(&group.id.as_str())
            || data
                .sources
                .iter()
                .any(|source| source.group_id.as_deref() == Some(group.id.as_str()))
    });
}

/// 把 records 切成按 source_id 的 map（前端用）
pub fn records_by_source<'a, I>(records: I) -> HashMap<String, Vec<StateRecord>>
where
    I: IntoIterator<Item = &'a StateRecord>,
{
    let mut m: HashMap<String, Vec<StateRecord>> = HashMap::new();
    for r in records {
        m.entry(r.source_id.clone()).or_default().push(r.clone());
    }
    m
}

fn write_temp_then_replace(path: &Path, content: String) -> anyhow::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

/// serde 兼容模块：VecDeque 序列化为 JSON 数组，反序列化时也接受数组
mod vecdeque_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::VecDeque;

    pub fn serialize<T, S>(deque: &VecDeque<T>, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: Serialize,
        S: Serializer,
    {
        let vec: Vec<&T> = deque.iter().collect();
        vec.serialize(serializer)
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<VecDeque<T>, D::Error>
    where
        T: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        let vec: Vec<T> = Vec::deserialize(deserializer)?;
        Ok(vec.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_state_serde_backward_compat() {
        let old_json = r#"{"person":true,"light":false,"frame_seq":42,"confidence":0.8}"#;
        let state: SceneState = serde_json::from_str(old_json).unwrap();
        assert!(state.person);
        assert!(!state.light);
        assert_eq!(state.frame_seq, 42);
        assert_eq!(state.confidence, 0.8);
        // 新字段取默认值
        assert_eq!(state.source, "");
        assert_eq!(state.person_confidence, 0.0);
        assert_eq!(state.light_confidence, 0.0);
        assert!(state.reason.is_none());
        assert!(state.model_latency_ms.is_none());
        assert_eq!(state.light_brightness, 0.0);
        assert_eq!(state.color_score, 0.0);
        assert_eq!(state.motion_score, 0.0);
        assert_eq!(state.process_ms, 0.0);
    }

    #[test]
    fn scene_state_serde_roundtrip() {
        let state = SceneState {
            person: true,
            light: false,
            frame_seq: 10,
            confidence: 0.9,
            source: "simple".into(),
            person_confidence: 0.75,
            light_confidence: 0.85,
            reason: Some("simple_motion_proxy".into()),
            model_latency_ms: Some(12),
            light_brightness: 88.0,
            color_score: 0.09,
            motion_score: 0.12,
            process_ms: 2.5,
        };
        let json = serde_json::to_string(&state).unwrap();
        let restored: SceneState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.source, "simple");
        assert_eq!(restored.person_confidence, 0.75);
        assert_eq!(restored.light_confidence, 0.85);
        assert_eq!(restored.reason.as_deref(), Some("simple_motion_proxy"));
        assert_eq!(restored.model_latency_ms, Some(12));
        assert_eq!(restored.light_brightness, 88.0);
        assert_eq!(restored.color_score, 0.09);
        assert_eq!(restored.motion_score, 0.12);
        assert_eq!(restored.process_ms, 2.5);
    }

    #[test]
    fn roi_config_migrates_legacy_on_off_thresholds() {
        let old_json = r#"{
            "sourceId": "src-test",
            "version": "roi-legacy",
            "lightOnThreshold": 0.04,
            "lightOffThreshold": 0.02,
            "updatedAt": 1000
        }"#;
        let mut cfg: RoiConfig = serde_json::from_str(old_json).unwrap();
        cfg.apply_legacy_threshold_migration();
        assert!((cfg.light_threshold - 0.04).abs() < f32::EPSILON);

        // 序列化后不应再出现旧字段。
        let serialized = serde_json::to_string(&cfg).unwrap();
        assert!(!serialized.contains("lightOnThreshold"));
        assert!(!serialized.contains("lightOffThreshold"));
        assert!(serialized.contains("lightThreshold"));
    }
}
