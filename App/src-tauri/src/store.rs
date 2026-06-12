// 视频源数据模型 + 持久化 + 分组 + 状态历史
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use uuid::Uuid;

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
    pub records: Vec<StateRecord>,
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
    fs::write(path, s)?;
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
    fs::write(path, s)?;
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
pub fn records_by_source(records: &[StateRecord]) -> HashMap<String, Vec<StateRecord>> {
    let mut m: HashMap<String, Vec<StateRecord>> = HashMap::new();
    for r in records {
        m.entry(r.source_id.clone()).or_default().push(r.clone());
    }
    m
}
