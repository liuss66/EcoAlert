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
    # 光源分析附加字段（light_source_filter_enabled=True 时有值）
    is_ceiling_light: bool = True   # 是否为天花板灯（非干扰源）
    interference_ratio: float = 0.0 # 干扰光源占比（0..1）
    light_source_cx: float = 0.5    # 亮度质心 x（归一化 0..1）
    light_source_cy: float = 0.5    # 亮度质心 y（归一化 0..1）


@dataclass
class LightSourceResult:
    """光源分析结果"""
    is_ceiling_light: bool = True
    interference_ratio: float = 0.0
    light_source_position: Tuple[float, float] = (0.5, 0.5)


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
#  光源定位分析：区分天花板灯与干扰光源（台灯/显示器）
# ============================================================

class LightSourceAnalyzer:
    """
    通过分析亮度的空间分布特征，区分天花板灯（目标）和台灯/显示器（干扰源）。

    三大特征：
    1. 亮度加权质心位置 — 天花板灯偏上，台灯偏下
    2. 峰值/均值比 — 天花板灯均匀(1~2x)，台灯集中(3~10x+)
    3. 连通域分布 — 天花板灯分散多区，台灯集中单点

    资源消耗：
    - 积分图 × 3 + 连通域分析，约 0.2~0.5ms（160×120）
    """

    def __init__(
        self,
        ceiling_height_ratio: float = 0.5,      # 上下分区线
        peak_mean_ratio_thresh: float = 3.0,    # 集中度阈值
        bright_thresh: int = 180,               # 亮像素分割阈值
        interference_smooth_alpha: float = 0.4, # EMA 平滑系数
        min_brightness: float = 50.0,           # 低于此亮度跳过分析
    ):
        self.ceiling_height_ratio = ceiling_height_ratio
        self.peak_mean_ratio_thresh = peak_mean_ratio_thresh
        self.bright_thresh = bright_thresh
        self.interference_smooth_alpha = interference_smooth_alpha
        self.min_brightness = min_brightness

        # 内部状态
        self._smooth_interference: float = 0.0
        self._initialized: bool = False

    def reset(self):
        self._smooth_interference = 0.0
        self._initialized = False

    def analyze(
        self,
        gray_frame: np.ndarray,
        rois: Optional[List[Rect]] = None,
    ) -> LightSourceResult:
        """
        分析光源类型，返回 LightSourceResult。

        Parameters
        ----------
        gray_frame : np.ndarray
            灰度图，uint8
        rois : list of Rect
            灯光检测 ROI 列表，None 则用全图

        Returns
        -------
        LightSourceResult
            is_ceiling_light, interference_ratio, light_source_position
        """
        h, w = gray_frame.shape[:2]
        if rois is None:
            rois = [Rect(0.0, 0.0, 1.0, 1.0)]

        # 提取各 ROI 区域
        regions: List[np.ndarray] = []
        for roi in rois:
            x1, y1, x2, y2 = roi.to_pixel(w, h)
            if x2 <= x1 or y2 <= y1:
                continue
            regions.append(gray_frame[y1:y2, x1:x2])

        if not regions:
            return LightSourceResult()

        # 早期退出：整体亮度太低，不值得分析
        overall_mean = sum(float(r.mean()) for r in regions) / len(regions)
        if overall_mean < self.min_brightness:
            return LightSourceResult()

        # 对每个 ROI 分析，按亮度×面积加权合并
        weighted_cx = 0.0
        weighted_cy = 0.0
        weighted_interference = 0.0
        total_weight = 0.0

        for region in regions:
            if region.size < 16:  # 太小的 ROI 跳过
                continue
            result = self._analyze_region(region)
            weight = float(region.mean()) * region.size
            weighted_cx += result.light_source_position[0] * weight
            weighted_cy += result.light_source_position[1] * weight
            weighted_interference += result.interference_ratio * weight
            total_weight += weight

        if total_weight < 1e-6:
            return LightSourceResult()

        cx = weighted_cx / total_weight
        cy = weighted_cy / total_weight
        interference = weighted_interference / total_weight

        # EMA 时间平滑
        if not self._initialized:
            self._smooth_interference = interference
            self._initialized = True
        else:
            self._smooth_interference = (
                self.interference_smooth_alpha * interference
                + (1.0 - self.interference_smooth_alpha) * self._smooth_interference
            )

        is_ceiling = self._smooth_interference < 0.5

        return LightSourceResult(
            is_ceiling_light=is_ceiling,
            interference_ratio=self._smooth_interference,
            light_source_position=(cx, cy),
        )

    def _analyze_region(self, region: np.ndarray) -> LightSourceResult:
        """
        分析单个 ROI 区域的光源特征。

        Returns
        -------
        LightSourceResult
            未平滑的原始结果
        """
        h, w = region.shape[:2]
        region_f = region.astype(np.float32)

        # ---- 特征 A：亮度加权质心（积分图）----
        # 使用 cv2.integral 计算 O(1) 矩
        integral = cv2.integral(region_f)  # (h+1, w+1)
        # 计算 x 和 y 的加权积分图
        ys, xs = np.mgrid[0:h, 0:w].astype(np.float32)
        integral_sx = cv2.integral(region_f * xs)
        integral_sy = cv2.integral(region_f * ys)

        total_mass = integral[h, w]
        if total_mass < 1e-6:
            return LightSourceResult()

        cx = integral_sx[h, w] / total_mass / w  # 归一化到 0..1
        cy = integral_sy[h, w] / total_mass / h  # 归一化到 0..1

        # ---- 特征 B：峰值/均值比（集中度）----
        mean_val = float(region.mean())
        peak_val = float(region.max())
        peak_mean_ratio = peak_val / max(mean_val, 1.0)

        # ---- 特征 C：连通域分析 ----
        binary = (region > self.bright_thresh).astype(np.uint8)
        # 可选：形态学开运算去噪
        kernel = np.ones((3, 3), np.uint8)
        binary = cv2.morphologyEx(binary, cv2.MORPH_OPEN, kernel)

        num_labels, labels, stats, centroids = cv2.connectedComponentsWithStats(
            binary, connectivity=8
        )
        # 排除背景（label=0）
        num_components = num_labels - 1

        largest_centroid_y = 0.5
        largest_area = 0
        total_bright_area = 0
        if num_components > 0:
            for i in range(1, num_labels):
                area = stats[i, cv2.CC_STAT_AREA]
                total_bright_area += area
                if area > largest_area:
                    largest_area = area
                    largest_centroid_y = centroids[i, 1] / h

        # ---- 综合评分 ----
        ceiling_score = 0.0
        interference_score = 0.0
        chr_ = self.ceiling_height_ratio

        # 特征 A 权重：0.45（垂直位置）
        if cy < chr_:
            ceiling_score += 0.45 * (1.0 - cy / chr_)
        else:
            interference_score += 0.45 * min((cy - chr_) / max(1.0 - chr_, 1e-6), 1.0)

        # 特征 B 权重：0.25（集中度）
        pmrt = self.peak_mean_ratio_thresh
        if peak_mean_ratio < pmrt:
            ceiling_score += 0.25 * (1.0 - peak_mean_ratio / pmrt)
        else:
            interference_score += 0.25 * min(peak_mean_ratio / pmrt - 1.0, 1.0)

        # 特征 C 权重：0.30（连通域）
        if num_components >= 3:
            # 多个亮区 → 天花板灯板
            ceiling_score += 0.15
        if num_components == 1 and largest_centroid_y > chr_:
            # 单个亮区在下半部 → 干扰
            interference_score += 0.20
        total_pixels = h * w
        if num_components == 1 and total_bright_area > 0:
            area_ratio = largest_area / total_bright_area
            size_ratio = largest_area / total_pixels
            if area_ratio > 0.8 and size_ratio < 0.1:
                # 单个小面积集中亮区 → 干扰
                interference_score += 0.10

        # 计算干扰比
        total_score = ceiling_score + interference_score
        if total_score < 1e-6:
            interference_ratio = 0.0
        else:
            interference_ratio = interference_score / total_score

        return LightSourceResult(
            is_ceiling_light=ceiling_score >= interference_score,
            interference_ratio=interference_ratio,
            light_source_position=(float(cx), float(cy)),
        )


# ============================================================
#  场景分析器（组合灯光 + 有人）
# ============================================================

class SceneAnalyzer:
    """
    组合 LightDetector + MotionDetector 输出 SceneState。
    可选启用 LightSourceAnalyzer 过滤干扰光源。
    """

    def __init__(
        self,
        light_rois: Optional[List[Rect]] = None,
        light_on_threshold: float = 140.0,
        light_off_threshold: float = 90.0,
        motion_low_res: Tuple[int, int] = (160, 120),
        motion_pixel_thresh: int = 20,
        motion_area_thresh: float = 0.03,
        # 光源过滤参数
        light_source_filter_enabled: bool = False,
        interference_threshold: float = 0.6,
        ceiling_height_ratio: float = 0.5,
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

        # 光源分析器（可选）
        self._light_analyzer: Optional[LightSourceAnalyzer] = None
        self._interference_threshold = interference_threshold
        if light_source_filter_enabled:
            self._light_analyzer = LightSourceAnalyzer(
                ceiling_height_ratio=ceiling_height_ratio,
            )

    def reset(self):
        self._light.reset()
        self._motion.reset()
        if self._light_analyzer:
            self._light_analyzer.reset()
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

        # 光源分析（可选）
        is_ceiling_light = True
        interference_ratio = 0.0
        light_source_cx = 0.5
        light_source_cy = 0.5

        if self._light_analyzer is not None and light_on:
            result = self._light_analyzer.analyze(gray, self._light.light_rois)
            is_ceiling_light = result.is_ceiling_light
            interference_ratio = result.interference_ratio
            light_source_cx, light_source_cy = result.light_source_position
            # 干扰超过阈值时覆盖灯光状态
            if interference_ratio > self._interference_threshold:
                light_on = False

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
            is_ceiling_light=is_ceiling_light,
            interference_ratio=interference_ratio,
            light_source_cx=light_source_cx,
            light_source_cy=light_source_cy,
        )

    @property
    def light_detector(self) -> LightDetector:
        return self._light

    @property
    def motion_detector(self) -> MotionDetector:
        return self._motion

    @property
    def light_source_analyzer(self) -> Optional[LightSourceAnalyzer]:
        return self._light_analyzer
