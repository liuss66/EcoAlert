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
    pub created_at: i64,
}

impl SourceGroup {
    pub fn new(name: impl Into<String>, order: i32) -> Self {
        Self {
            id: format!("grp-{}", Uuid::new_v4().simple()),
            name: name.into(),
            order,
            collapsed: false,
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
    pub person_threshold: f32,
    pub light_threshold: f32,
    pub alarm_hold_sec: u32,
    pub alarm_recover_sec: u32,
    pub recover_policy: String,
    pub vlm_hourly_limit: u32,
    #[serde(default)]
    pub roi_version: Option<String>,
}

impl Default for AlgorithmConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            scope: "global".into(),
            scope_id: None,
            active_windows: vec![ActiveWindow {
                weekdays: vec![1, 2, 3, 4, 5],
                start: "18:30".into(),
                end: "08:30".into(),
                timezone: default_timezone(),
            }],
            exception_windows: vec![],
            simple_interval_sec: 10,
            vlm_interval_sec: 300,
            vlm_enabled: false,
            vlm_skip_when_person: true,
            person_threshold: 0.65,
            light_threshold: 0.70,
            alarm_hold_sec: 300,
            alarm_recover_sec: 60,
            recover_policy: "either".into(),
            vlm_hourly_limit: 12,
            roi_version: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlgorithmConfigFile {
    pub schema_version: u32,
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
    pub light_on_threshold: f32,
    pub light_off_threshold: f32,
    pub updated_at: i64,
}

impl RoiConfig {
    pub fn new(source_id: String) -> Self {
        Self {
            source_id,
            version: format!("roi-{}", Uuid::new_v4().simple()),
            light_rois: vec![],
            exclude_rois: vec![],
            person_rois: vec![],
            light_on_threshold: 0.70,
            light_off_threshold: 0.45,
            updated_at: chrono::Utc::now().timestamp_millis(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoiConfigFile {
    pub schema_version: u32,
    #[serde(default)]
    pub by_source: HashMap<String, RoiConfig>,
}

impl Default for RoiConfigFile {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            by_source: HashMap::new(),
        }
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
