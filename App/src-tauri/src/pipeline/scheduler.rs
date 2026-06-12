//! 算法调度骨架。
//!
//! 当前只负责配置继承、启用时段判断和跳过原因。后续接入真实算法时，
//! detector / analyzer 只处理识别逻辑，调度决策仍集中在这里。

use crate::store::{ActiveWindow, AlgorithmConfig, AlgorithmConfigFile, VideoSource};
use chrono::{Datelike, Local, NaiveTime, Timelike};

#[derive(Debug, Clone)]
pub struct ScheduleDecision {
    pub should_run_simple: bool,
    pub should_run_vlm: bool,
    pub reason: String,
    pub config_scope: String,
    pub effective_config: AlgorithmConfig,
}

#[derive(Debug, Clone)]
pub struct EffectiveAlgorithmConfig {
    pub config: AlgorithmConfig,
    pub scope: String,
}

pub fn effective_algorithm_config(
    source: &VideoSource,
    config_file: &AlgorithmConfigFile,
) -> EffectiveAlgorithmConfig {
    if let Some(config) = config_file.sources.get(&source.id) {
        return EffectiveAlgorithmConfig {
            config: config.clone(),
            scope: "source".into(),
        };
    }

    if let Some(group_id) = &source.group_id {
        if let Some(config) = config_file.groups.get(group_id) {
            return EffectiveAlgorithmConfig {
                config: config.clone(),
                scope: "group".into(),
            };
        }
    }

    EffectiveAlgorithmConfig {
        config: config_file.global.clone(),
        scope: "global".into(),
    }
}

pub fn decide_for_source(
    source: &VideoSource,
    config_file: &AlgorithmConfigFile,
) -> ScheduleDecision {
    let effective = effective_algorithm_config(source, config_file);
    let config = effective.config.clone();

    if !source.enabled {
        return ScheduleDecision {
            should_run_simple: false,
            should_run_vlm: false,
            reason: "source_disabled".into(),
            config_scope: effective.scope,
            effective_config: config,
        };
    }

    if !config.enabled {
        return ScheduleDecision {
            should_run_simple: false,
            should_run_vlm: false,
            reason: "algorithm_disabled".into(),
            config_scope: effective.scope,
            effective_config: config,
        };
    }

    if in_any_window(&config.exception_windows) {
        return ScheduleDecision {
            should_run_simple: false,
            should_run_vlm: false,
            reason: "exception_window".into(),
            config_scope: effective.scope,
            effective_config: config,
        };
    }

    if !config.active_windows.is_empty() && !in_any_window(&config.active_windows) {
        return ScheduleDecision {
            should_run_simple: false,
            should_run_vlm: false,
            reason: "schedule_disabled".into(),
            config_scope: effective.scope,
            effective_config: config,
        };
    }

    ScheduleDecision {
        should_run_simple: true,
        should_run_vlm: config.vlm_enabled,
        reason: "scheduled".into(),
        config_scope: effective.scope,
        effective_config: config,
    }
}

fn in_any_window(windows: &[ActiveWindow]) -> bool {
    windows.iter().any(is_now_in_window)
}

fn is_now_in_window(window: &ActiveWindow) -> bool {
    let now = Local::now();
    let weekday = now.weekday().number_from_monday() as u8;
    if !window.weekdays.is_empty() && !window.weekdays.contains(&weekday) {
        return false;
    }

    let Some(start) = parse_hhmm(&window.start) else {
        return false;
    };
    let Some(end) = parse_hhmm(&window.end) else {
        return false;
    };
    let current =
        NaiveTime::from_hms_opt(now.hour(), now.minute(), now.second()).unwrap_or(NaiveTime::MIN);

    if start <= end {
        current >= start && current <= end
    } else {
        current >= start || current <= end
    }
}

fn parse_hhmm(value: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(value, "%H:%M").ok()
}
