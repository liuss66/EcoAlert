//! 告警规则（占位）
//!
//! 计划：DSL / 表达式 → 触发条件 → 冷却时间 → 推送
//! 第一版只做硬编码规则：
//! - motion 连续 5 帧 → warn
//! - 目标置信度 > 0.9 → info
//! - 通道掉线 > 30 秒 → critical

use crate::pipeline::detector::Detection;
use std::time::{Duration, Instant};

pub struct AlertRule {
    pub last_fired: Option<Instant>,
    pub cooldown: Duration,
}

impl AlertRule {
    pub fn new(cooldown_ms: u64) -> Self {
        Self { last_fired: None, cooldown: Duration::from_millis(cooldown_ms) }
    }

    /// 触发告警，遵守冷却时间
    pub fn fire(&mut self, det: &Detection) -> Option<(String, String)> {
        let now = Instant::now();
        if let Some(last) = self.last_fired {
            if now.duration_since(last) < self.cooldown { return None; }
        }
        self.last_fired = Some(now);
        Some((
            if det.confidence > 0.9 { "info".into() } else { "warn".into() },
            format!("检测到 {} (conf={:.2})", det.kind, det.confidence),
        ))
    }
}
