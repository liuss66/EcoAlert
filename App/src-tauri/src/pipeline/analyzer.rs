//! 帧级分析（占位）
//!
//! 输入：检测结果序列
//! 输出：聚合统计 + 异常判断
//! - 滑动窗口内目标数量
//! - 滞留时间（同一位置目标停留多久）
//! - 流量统计（人来人往密度）

use crate::pipeline::detector::Detection;
use std::collections::VecDeque;

pub struct Analyzer {
    /// 最近 N 个检测结果
    window: VecDeque<Detection>,
    window_size: usize,
}

impl Analyzer {
    pub fn new() -> Self {
        Self { window: VecDeque::with_capacity(128), window_size: 128 }
    }

    pub fn feed(&mut self, det: Detection) {
        if self.window.len() >= self.window_size {
            self.window.pop_front();
        }
        self.window.push_back(det);
    }

    pub fn window_count(&self) -> usize {
        self.window.len()
    }

    /// 简单异常判断：窗口内 motion 数量超阈值
    pub fn is_bursty(&self, threshold: usize) -> bool {
        self.window.iter().filter(|d| d.kind == "motion").count() > threshold
    }
}

impl Default for Analyzer {
    fn default() -> Self { Self::new() }
}
