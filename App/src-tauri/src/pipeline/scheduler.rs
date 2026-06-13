//! 算法调度骨架。
//!
//! 当前只负责配置继承、启用时段判断和跳过原因。后续接入真实算法时，
//! detector / analyzer 只处理识别逻辑，调度决策仍集中在这里。

use crate::store::{ActiveWindow, AlgorithmConfig, AlgorithmConfigFile, VideoSource};
use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, NaiveTime, Timelike};

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
    decide_for_source_at(source, config_file, Local::now())
}

fn decide_for_source_at(
    source: &VideoSource,
    config_file: &AlgorithmConfigFile,
    now: DateTime<Local>,
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

    if in_any_window_at(&config.exception_windows, now) {
        return ScheduleDecision {
            should_run_simple: false,
            should_run_vlm: false,
            reason: "exception_window".into(),
            config_scope: effective.scope,
            effective_config: config,
        };
    }

    if !config.active_windows.is_empty() && !in_any_window_at(&config.active_windows, now) {
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

fn in_any_window_at(windows: &[ActiveWindow], now: DateTime<Local>) -> bool {
    windows.iter().any(|window| is_time_in_window(window, now))
}

fn is_time_in_window(window: &ActiveWindow, now: DateTime<Local>) -> bool {
    let Some(start) = parse_hhmm(&window.start) else {
        return false;
    };
    let Some(end) = parse_hhmm(&window.end) else {
        return false;
    };
    let current =
        NaiveTime::from_hms_opt(now.hour(), now.minute(), now.second()).unwrap_or(NaiveTime::MIN);

    if start <= end {
        let weekday = now.weekday().number_from_monday() as u8;
        if !window.weekdays.is_empty() && !window.weekdays.contains(&weekday) {
            return false;
        }
        current >= start && current <= end
    } else {
        let window_weekday = if current <= end {
            (now - ChronoDuration::days(1))
                .weekday()
                .number_from_monday() as u8
        } else {
            now.weekday().number_from_monday() as u8
        };
        if !window.weekdays.is_empty() && !window.weekdays.contains(&window_weekday) {
            return false;
        }
        current >= start || current <= end
    }
}

fn parse_hhmm(value: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(value, "%H:%M").ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    fn source(id: &str, enabled: bool, group_id: Option<&str>) -> VideoSource {
        VideoSource {
            id: id.into(),
            name: id.into(),
            url: "http://127.0.0.1:8080/cam-1/index.m3u8".into(),
            source_type: "hls".into(),
            location: String::new(),
            enabled,
            group_id: group_id.map(str::to_string),
            order: 0,
            created_at: 0,
        }
    }

    fn window(weekdays: Vec<u8>, start: &str, end: &str) -> ActiveWindow {
        ActiveWindow {
            weekdays,
            start: start.into(),
            end: end.into(),
            timezone: "Local".into(),
        }
    }

    fn local_dt(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(y, m, d, h, min, 0)
            .single()
            .expect("valid local test datetime")
    }

    #[test]
    fn effective_config_prefers_source_then_group_then_global() {
        let src = source("src-1", true, Some("grp-1"));
        let mut file = AlgorithmConfigFile::default();
        file.global.simple_interval_sec = 10;

        let mut group_cfg = file.global.clone();
        group_cfg.simple_interval_sec = 20;
        file.groups.insert("grp-1".into(), group_cfg);

        let effective = effective_algorithm_config(&src, &file);
        assert_eq!(effective.scope, "group");
        assert_eq!(effective.config.simple_interval_sec, 20);

        let mut source_cfg = file.global.clone();
        source_cfg.simple_interval_sec = 30;
        file.sources.insert("src-1".into(), source_cfg);

        let effective = effective_algorithm_config(&src, &file);
        assert_eq!(effective.scope, "source");
        assert_eq!(effective.config.simple_interval_sec, 30);
    }

    #[test]
    fn disabled_source_skips_before_algorithm_config() {
        let src = source("src-1", false, None);
        let mut file = AlgorithmConfigFile::default();
        file.global.enabled = false;
        let decision = decide_for_source_at(&src, &file, local_dt(2026, 6, 15, 20, 0));
        assert!(!decision.should_run_simple);
        assert_eq!(decision.reason, "source_disabled");
    }

    #[test]
    fn active_window_supports_cross_day_schedule() {
        let src = source("src-1", true, None);
        let mut file = AlgorithmConfigFile::default();
        file.global.active_windows = vec![window(vec![1], "18:30", "08:30")];

        let monday_night = decide_for_source_at(&src, &file, local_dt(2026, 6, 15, 20, 0));
        assert!(monday_night.should_run_simple);

        let tuesday_morning = decide_for_source_at(&src, &file, local_dt(2026, 6, 16, 7, 0));
        assert!(tuesday_morning.should_run_simple);

        let monday_morning = decide_for_source_at(&src, &file, local_dt(2026, 6, 15, 7, 0));
        assert!(!monday_morning.should_run_simple);
        assert_eq!(monday_morning.reason, "schedule_disabled");

        let monday_noon = decide_for_source_at(&src, &file, local_dt(2026, 6, 15, 12, 0));
        assert!(!monday_noon.should_run_simple);
        assert_eq!(monday_noon.reason, "schedule_disabled");
    }

    #[test]
    fn cross_day_weekday_uses_start_day_for_next_morning() {
        let src = source("src-1", true, None);
        let mut file = AlgorithmConfigFile::default();
        file.global.active_windows = vec![window(vec![5], "18:30", "08:30")];

        let saturday_morning = decide_for_source_at(&src, &file, local_dt(2026, 6, 20, 7, 0));
        assert!(saturday_morning.should_run_simple);

        let friday_morning = decide_for_source_at(&src, &file, local_dt(2026, 6, 19, 7, 0));
        assert!(!friday_morning.should_run_simple);
    }

    #[test]
    fn exception_window_overrides_active_window() {
        let src = source("src-1", true, None);
        let mut file = AlgorithmConfigFile::default();
        file.global.active_windows = vec![window(vec![1], "18:30", "08:30")];
        file.global.exception_windows = vec![window(vec![1], "19:00", "21:00")];

        let decision = decide_for_source_at(&src, &file, local_dt(2026, 6, 15, 20, 0));
        assert!(!decision.should_run_simple);
        assert_eq!(decision.reason, "exception_window");
    }
}
