"""
EcoAlert 轻量算法模块
=====================
两个超低资源消耗的视觉检测算法，用于验证"是否开灯 / 是否有人"。

算法特点
--------
- 无深度学习模型，纯图像处理，CPU 单帧处理 < 1ms（160×120 灰度）
- 支持 ROI 区域检测，避开屏幕、窗户等干扰源
- 磁滞阈值 + 时间平滑，避免状态频繁跳变
- 内存占用 < 5MB

算法说明
--------
1. LightDetector  — ROI 亮度均值 + 磁滞阈值
   - 对每个 light_roi 计算灰度均值，整体取平均
   - 均值 > light_on_threshold  → 灯亮
   - 均值 < light_off_threshold → 灯灭
   - 中间区域保持前一状态（磁滞）
   - 指数移动平均（EMA）时间平滑

2. MotionDetector  — 帧差法 + 降采样
   - 输入帧先缩放到 low_res（默认 160×120）
   - 计算当前帧与上一帧的绝对差
   - 差值超过 pixel_thresh 的像素比例 > area_ratio_thresh → 有人
   - 对运动值做 EMA 时间平滑

3. SceneAnalyzer  — 组合两路输出
   - 输出 SceneState { person, light, confidence }

数据结构与主 App 的 SceneState 对齐：
   person: bool,  light: bool,  confidence: f32
"""

from __future__ import annotations

import time
from dataclasses import dataclass, field
from typing import List, Optional, Tuple

import cv2
import numpy as np


# ============================================================
#  数据结构
# ============================================================

@dataclass
class Rect:
    """归一化 ROI 矩形 [0..1]"""
    x: float
    y: float
    w: float
    h: float

    def to_pixel(self, width: int, height: int) -> Tuple[int, int, int, int]:
        x1 = int(max(0.0, self.x) * width)
        y1 = int(max(0.0, self.y) * height)
        x2 = int(min(1.0, self.x + self.w) * width)
        y2 = int(min(1.0, self.y + self.h) * height)
        return x1, y1, x2, y2


@dataclass
class SceneState:
    """单帧检测结果，与 App SceneState 对齐"""
    person: bool = False
    light: bool = False
    light_brightness: float = 0.0   # 附加：ROI 平均亮度（0..255）
    motion_score: float = 0.0       # 附加：运动强度（0..1）
    frame_seq: int = 0
    confidence: float = 0.0
    process_ms: float = 0.0         # 附加：单帧耗时（ms）


# ============================================================
#  灯光检测：ROI 亮度均值 + 磁滞阈值
# ============================================================

class LightDetector:
    """
    通过 ROI 区域灰度均值判断灯是否开启。

    资源消耗：
    - 仅对 ROI 小区域求均值，O(roi_pixels) 每帧
    - 160×120 灰度图下全图均值约 0.02ms
    """

    def __init__(
        self,
        light_rois: Optional[List[Rect]] = None,
        light_on_threshold: float = 140.0,   # 亮度均值 > 此值 → 灯亮
        light_off_threshold: float = 90.0,    # 亮度均值 < 此值 → 灯灭
        smooth_alpha: float = 0.3,            # EMA 平滑系数（越小越平滑）
    ):
        self.light_rois = light_rois or [Rect(0.0, 0.0, 1.0, 1.0)]
        self.light_on_threshold = light_on_threshold
        self.light_off_threshold = light_off_threshold
        self.smooth_alpha = smooth_alpha

        # 内部状态
        self._smooth_brightness: float = 0.0
        self._light_on: bool = False
        self._initialized: bool = False

    def reset(self):
        self._smooth_brightness = 0.0
        self._light_on = False
        self._initialized = False

    def process(self, gray_frame: np.ndarray) -> Tuple[bool, float]:
        """
        处理一帧灰度图，返回 (light_on, raw_brightness)。

        Parameters
        ----------
        gray_frame : np.ndarray
            灰度图，uint8，任意尺寸

        Returns
        -------
        (bool, float)
            light_on: 灯是否亮
            raw_brightness: ROI 平均亮度（0..255）
        """
        h, w = gray_frame.shape[:2]

        # 计算各 ROI 平均亮度
        roi_means: List[float] = []
        for roi in self.light_rois:
            x1, y1, x2, y2 = roi.to_pixel(w, h)
            if x2 <= x1 or y2 <= y1:
                continue
            region = gray_frame[y1:y2, x1:x2]
            roi_means.append(float(region.mean()))

        if not roi_means:
            # 无有效 ROI，用全图
            raw_brightness = float(gray_frame.mean())
        else:
            raw_brightness = sum(roi_means) / len(roi_means)

        # EMA 时间平滑
        if not self._initialized:
            self._smooth_brightness = raw_brightness
            self._initialized = True
        else:
            self._smooth_brightness = (
                self.smooth_alpha * raw_brightness
                + (1.0 - self.smooth_alpha) * self._smooth_brightness
            )

        # 磁滞阈值判定
        brightness = self._smooth_brightness
        if brightness > self.light_on_threshold:
            self._light_on = True
        elif brightness < self.light_off_threshold:
            self._light_on = False
        # 中间区域保持不变

        return self._light_on, raw_brightness


# ============================================================
#  有人检测：帧差法（运动侦测）
# ============================================================

class MotionDetector:
    """
    帧差法运动检测，用于判断是否有人。

    资源消耗：
    - 降采样到低分辨率（默认 160×120）
    - 单帧 absdiff + mean，约 0.05ms
    - 无模型、无迭代，纯向量运算

    优化点：
    - 不需要存储完整历史帧，只保留前一帧
    - 降采样后处理，像素量仅为原始帧的 1/16 ~ 1/64
    """

    def __init__(
        self,
        low_res: Tuple[int, int] = (160, 120),
        pixel_thresh: int = 20,         # 单帧差值阈值（0..255）
        area_ratio_thresh: float = 0.03,  # 变化像素占比超此值 → 有运动
        smooth_alpha: float = 0.3,
    ):
        self.low_res = low_res
        self.pixel_thresh = pixel_thresh
        self.area_ratio_thresh = area_ratio_thresh
        self.smooth_alpha = smooth_alpha

        self._prev_gray: Optional[np.ndarray] = None
        self._smooth_motion: float = 0.0
        self._motion_on: bool = False

    def reset(self):
        self._prev_gray = None
        self._smooth_motion = 0.0
        self._motion_on = False

    def process(self, gray_frame: np.ndarray) -> Tuple[bool, float]:
        """
        处理一帧灰度图，返回 (person_detected, motion_score)。

        Parameters
        ----------
        gray_frame : np.ndarray
            灰度图，uint8

        Returns
        -------
        (bool, float)
            person_detected: 是否检测到运动（视为有人）
            motion_score: 运动强度（0..1）
        """
        # 降采样，降低计算量
        small = cv2.resize(
            gray_frame, self.low_res, interpolation=cv2.INTER_AREA
        )
        small_f = small.astype(np.float32)

        motion_score = 0.0
        if self._prev_gray is not None:
            # 帧差：计算像素差
            diff = np.abs(small_f - self._prev_gray)
            changed = (diff > self.pixel_thresh).astype(np.float32)
            raw_ratio = float(changed.mean())

            # 归一化到 0..1（raw_ratio 通常在 0..0.3 之间）
            motion_score = min(raw_ratio / 0.15, 1.0)

            # EMA 平滑
            self._smooth_motion = (
                self.smooth_alpha * motion_score
                + (1.0 - self.smooth_alpha) * self._smooth_motion
            )

            # 判定
            self._motion_on = self._smooth_motion > self.area_ratio_thresh

        self._prev_gray = small_f
        return self._motion_on, motion_score


# ============================================================
#  场景分析器（组合灯光 + 有人）
# ============================================================

class SceneAnalyzer:
    """
    组合 LightDetector + MotionDetector 输出 SceneState。
    """

    def __init__(
        self,
        light_rois: Optional[List[Rect]] = None,
        light_on_threshold: float = 140.0,
        light_off_threshold: float = 90.0,
        motion_low_res: Tuple[int, int] = (160, 120),
        motion_pixel_thresh: int = 20,
        motion_area_thresh: float = 0.03,
    ):
        self._light = LightDetector(
            light_rois=light_rois,
            light_on_threshold=light_on_threshold,
            light_off_threshold=light_off_threshold,
        )
        self._motion = MotionDetector(
            low_res=motion_low_res,
            pixel_thresh=motion_pixel_thresh,
            area_ratio_thresh=motion_area_thresh,
        )
        self._frame_seq = 0

    def reset(self):
        self._light.reset()
        self._motion.reset()
        self._frame_seq = 0

    def process(self, frame: np.ndarray) -> SceneState:
        """
        处理一帧 BGR 图像，返回 SceneState。
        """
        t0 = time.perf_counter()
        self._frame_seq += 1

        gray = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)

        light_on, brightness = self._light.process(gray)
        person_on, motion_score = self._motion.process(gray)

        elapsed_ms = (time.perf_counter() - t0) * 1000.0

        # 综合置信度：两路信号越强，置信度越高
        light_conf = brightness / 255.0
        motion_conf = motion_score
        confidence = (light_conf + motion_conf) / 2.0

        return SceneState(
            person=person_on,
            light=light_on,
            light_brightness=brightness,
            motion_score=motion_score,
            frame_seq=self._frame_seq,
            confidence=confidence,
            process_ms=elapsed_ms,
        )

    @property
    def light_detector(self) -> LightDetector:
        return self._light

    @property
    def motion_detector(self) -> MotionDetector:
        return self._motion
