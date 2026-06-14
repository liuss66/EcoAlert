"""
EcoAlert App algorithm mirror for local testing.

This module intentionally mirrors the current Rust pipeline in App:
- light detection prefers RGB color score, with brightness fallback
- person detection is the current motion proxy
- VLM can fill missed person detections when enabled
- alarm state follows hold/recover timing and recover policy
"""

from __future__ import annotations

import base64
import json
import re
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from typing import Any, Optional

import cv2
import numpy as np


MOTION_WIDTH = 160
MOTION_HEIGHT = 120
MOTION_PIXEL_THRESHOLD = 20
MOTION_AREA_THRESHOLD = 0.03
EMA_ALPHA = 0.3
COLOR_ON_THRESHOLD = 0.055
COLOR_OFF_THRESHOLD = 0.025
COLOR_DARK_LUMA_CUTOFF = 24.0
COLOR_WEIGHT_LUMA_FLOOR = 40.0


@dataclass
class Rect:
    x: float
    y: float
    w: float
    h: float
    id: str = ""
    label: str = ""

    def bounds(self, width: int, height: int) -> Optional[tuple[int, int, int, int]]:
        if width <= 0 or height <= 0:
            return None
        x1 = int(np.floor(max(0.0, min(1.0, self.x)) * width))
        y1 = int(np.floor(max(0.0, min(1.0, self.y)) * height))
        x2 = int(np.ceil(max(0.0, min(1.0, self.x + self.w)) * width))
        y2 = int(np.ceil(max(0.0, min(1.0, self.y + self.h)) * height))
        if x2 <= x1 or y2 <= y1:
            return None
        return x1, y1, x2, y2


@dataclass
class RoiConfig:
    light_rois: list[Rect] = field(default_factory=list)
    light_on_threshold: float = COLOR_ON_THRESHOLD
    light_off_threshold: float = COLOR_OFF_THRESHOLD


@dataclass
class AlgorithmConfig:
    enabled: bool = True
    developer_mode: bool = True
    vlm_enabled: bool = False
    vlm_skip_when_person: bool = True
    vlm_interval_sec: int = 300
    vlm_hourly_limit: int = 12
    vlm_api_base: str = ""
    vlm_api_key: str = ""
    vlm_model: str = ""
    vlm_prompt: str = (
        "你是一个专业的人体目标检测系统。请严格输出 JSON："
        '{"has_person": true/false, "detections": [{"label": "person", '
        '"confidence": 0.95, "bbox": [x1, y1, x2, y2]}]}。'
    )
    vlm_temperature: float = 0.1
    vlm_max_tokens: int = 2048
    person_threshold: float = 0.65
    light_threshold: float = 0.70
    alarm_hold_sec: int = 300
    alarm_recover_sec: int = 60
    recover_policy: str = "either"


@dataclass
class SceneState:
    person: bool = False
    light: bool = False
    frame_seq: int = 0
    confidence: float = 0.0
    source: str = "simple"
    person_confidence: float = 0.0
    light_confidence: float = 0.0
    reason: Optional[str] = None
    model_latency_ms: Optional[int] = None
    light_brightness: float = 0.0
    color_score: float = 0.0
    motion_score: float = 0.0
    process_ms: float = 0.0


@dataclass
class AnalysisResult:
    scene: SceneState
    light_brightness: float
    motion_score: float
    process_ms: float


@dataclass
class VlmDetection:
    has_person: bool
    confidence: float
    raw: str


@dataclass
class RuntimeResult:
    state: SceneState
    raw_alarm: bool
    alarm_status: str
    alarm_transition: Optional[bool]
    schedule_reason: str
    vlm_reason: Optional[str] = None
    vlm_error: Optional[str] = None


class Detector:
    def __init__(self, person_threshold: float = 0.65, light_threshold: float = 0.70):
        self.prev_low_res: Optional[np.ndarray] = None
        self.light_state = False
        self.brightness_ema: Optional[float] = None
        self.color_ema: Optional[float] = None
        self.motion_ema = 0.0
        self.frame_counter = 0
        self.person_threshold = person_threshold
        self.light_threshold = light_threshold

    def reset(self) -> None:
        self.prev_low_res = None
        self.light_state = False
        self.brightness_ema = None
        self.color_ema = None
        self.motion_ema = 0.0
        self.frame_counter = 0

    def set_thresholds(self, person_threshold: float, light_threshold: float) -> None:
        self.person_threshold = person_threshold
        self.light_threshold = light_threshold

    def effective_person_threshold(self) -> float:
        return max(float(np.clip(self.person_threshold, 0.05, 1.0)) * MOTION_AREA_THRESHOLD, 0.001)

    def analyze_scene(self, frame_bgr: np.ndarray, roi_config: Optional[RoiConfig] = None) -> AnalysisResult:
        started = time.perf_counter()
        self.frame_counter += 1
        gray = _to_gray(frame_bgr)
        rgb = _to_rgb(frame_bgr)

        brightness = average_light_brightness(gray, roi_config)
        smoothed_brightness = _ema(self.brightness_ema, brightness)
        self.brightness_ema = smoothed_brightness

        color_score = average_color_score(rgb, gray, roi_config)
        smoothed_color = _ema(self.color_ema, color_score)
        self.color_ema = smoothed_color

        has_color_signal = rgb.size > 0
        color_on_threshold, color_off_threshold = color_thresholds(roi_config)
        brightness_on_threshold = self.light_threshold
        brightness_off_threshold = self.light_threshold * 0.65
        brightness_norm = float(np.clip(smoothed_brightness / 255.0, 0.0, 1.0))

        if has_color_signal:
            if smoothed_color >= color_on_threshold:
                self.light_state = True
            elif smoothed_color <= color_off_threshold:
                self.light_state = False
        else:
            if brightness_norm >= brightness_on_threshold:
                self.light_state = True
            elif brightness_norm <= brightness_off_threshold:
                self.light_state = False

        motion_raw = self.motion_score(gray)
        self.motion_ema = self.motion_ema * (1.0 - EMA_ALPHA) + motion_raw * EMA_ALPHA
        effective_threshold = self.effective_person_threshold()
        person = effective_threshold > 0.0 and self.motion_ema >= effective_threshold
        if effective_threshold > 0.0:
            if person:
                person_confidence = 0.5 + 0.5 * min((self.motion_ema - effective_threshold) / effective_threshold, 1.0)
            else:
                person_confidence = 0.5 * (self.motion_ema / effective_threshold)
        else:
            person_confidence = 0.0

        if has_color_signal:
            light_conf = color_light_confidence(
                smoothed_color, self.light_state, color_on_threshold, color_off_threshold
            )
            light_by = "color"
        else:
            light_conf = light_confidence(
                brightness_norm, self.light_state, brightness_on_threshold, brightness_off_threshold
            )
            light_by = "brightness"

        process_ms = (time.perf_counter() - started) * 1000.0
        reason = "simple_motion_proxy" if person else "simple_no_motion"
        scene = SceneState(
            person=person,
            light=self.light_state,
            frame_seq=self.frame_counter,
            confidence=light_conf,
            source="simple",
            person_confidence=float(np.clip(person_confidence, 0.0, 1.0)),
            light_confidence=light_conf,
            reason=f"{reason};light_by_{light_by}",
            model_latency_ms=int(process_ms),
            light_brightness=smoothed_brightness,
            color_score=smoothed_color,
            motion_score=self.motion_ema,
            process_ms=process_ms,
        )
        return AnalysisResult(scene, smoothed_brightness, self.motion_ema, process_ms)

    def motion_score(self, gray: np.ndarray) -> float:
        if gray.size == 0:
            return 0.0
        small = cv2.resize(
            gray,
            (min(MOTION_WIDTH, gray.shape[1]), min(MOTION_HEIGHT, gray.shape[0])),
            interpolation=cv2.INTER_NEAREST,
        )
        if self.prev_low_res is None or self.prev_low_res.shape != small.shape:
            self.prev_low_res = small.copy()
            return 0.0
        diff = cv2.absdiff(small, self.prev_low_res)
        changed = int(np.count_nonzero(diff > MOTION_PIXEL_THRESHOLD))
        self.prev_low_res = small.copy()
        return changed / float(small.size)


class AlarmTimer:
    def __init__(self) -> None:
        self.alarm_since: Optional[int] = None
        self.recover_since: Optional[int] = None
        self.active = False

    def update(
        self,
        raw_alarm: bool,
        recover_condition: bool,
        now_ms: int,
        hold_sec: int,
        recover_sec: int,
    ) -> Optional[bool]:
        if raw_alarm:
            self.recover_since = None
            if self.alarm_since is None:
                self.alarm_since = now_ms
            if not self.active and now_ms - self.alarm_since >= hold_sec * 1000:
                self.active = True
                return True
            return None

        self.alarm_since = None
        if self.active:
            if recover_condition:
                if self.recover_since is None:
                    self.recover_since = now_ms
                if now_ms - self.recover_since >= recover_sec * 1000:
                    self.active = False
                    self.recover_since = None
                    return False
            else:
                self.recover_since = None
        else:
            self.recover_since = None
        return None


class SceneProcessor:
    def __init__(
        self,
        algorithm_config: Optional[AlgorithmConfig] = None,
        roi_config: Optional[RoiConfig] = None,
    ) -> None:
        self.config = algorithm_config or AlgorithmConfig()
        self.roi_config = roi_config or RoiConfig()
        self.detector = Detector(self.config.person_threshold, self.config.light_threshold)
        self.alarm_timer = AlarmTimer()
        self.last_vlm_run_ms: Optional[int] = None
        self.vlm_hour_bucket: Optional[int] = None
        self.vlm_hour_count = 0

    def reset(self) -> None:
        self.detector.reset()
        self.alarm_timer = AlarmTimer()
        self.last_vlm_run_ms = None
        self.vlm_hour_bucket = None
        self.vlm_hour_count = 0

    def process(self, frame_bgr: np.ndarray, now_ms: Optional[int] = None) -> RuntimeResult:
        now_ms = now_ms if now_ms is not None else int(time.time() * 1000)
        if not self.config.enabled:
            state = SceneState(reason="algorithm_disabled")
            return RuntimeResult(state, False, "normal", None, "algorithm_disabled")

        self.detector.set_thresholds(self.config.person_threshold, self.config.light_threshold)
        state = self.detector.analyze_scene(frame_bgr, self.roi_config).scene
        vlm_reason = None
        vlm_error = None

        if self._should_run_vlm(state, now_ms):
            self.last_vlm_run_ms = now_ms
            self.vlm_hour_count += 1
            try:
                vlm_result = analyze_person(self.config, frame_bgr)
                if vlm_result.has_person:
                    state.person = True
                    state.person_confidence = max(state.person_confidence, vlm_result.confidence)
                    state.confidence = max(state.confidence, vlm_result.confidence)
                    state.source = "fused"
                    state.reason = "vlm_person_detected"
                    vlm_reason = "vlm_person_detected"
                else:
                    vlm_reason = "vlm_no_person"
            except Exception as exc:
                vlm_error = f"VLM 检测失败: {exc}"
                vlm_reason = "vlm_error"

        raw_alarm = (not state.person) and state.light
        recover_condition = should_recover_alarm(state.person, state.light, self.config.recover_policy)
        transition = self.alarm_timer.update(
            raw_alarm,
            recover_condition,
            now_ms,
            self.config.alarm_hold_sec,
            self.config.alarm_recover_sec,
        )
        if self.alarm_timer.active:
            alarm_status = "alarm_active"
        elif raw_alarm:
            alarm_status = "suspected"
        else:
            alarm_status = "normal"
        return RuntimeResult(state, raw_alarm, alarm_status, transition, "run_simple", vlm_reason, vlm_error)

    def _should_run_vlm(self, state: SceneState, now_ms: int) -> bool:
        if not self.config.vlm_enabled:
            return False
        if self.config.vlm_skip_when_person and state.person:
            return False
        interval_ms = max(self.config.vlm_interval_sec, 30) * 1000
        if self.last_vlm_run_ms is not None and now_ms - self.last_vlm_run_ms < interval_ms:
            return False
        hour_bucket = now_ms // 3_600_000
        if self.vlm_hour_bucket != hour_bucket:
            self.vlm_hour_bucket = hour_bucket
            self.vlm_hour_count = 0
        return self.config.vlm_hourly_limit == 0 or self.vlm_hour_count < self.config.vlm_hourly_limit


SceneAnalyzer = SceneProcessor


def should_recover_alarm(person: bool, light: bool, policy: str) -> bool:
    if policy == "light_off":
        return not light
    if policy == "person_present":
        return person
    if policy == "both":
        return person and not light
    return person or not light


def analyze_person(config: AlgorithmConfig, frame_bgr: np.ndarray) -> VlmDetection:
    api_base = config.vlm_api_base.strip().rstrip("/")
    api_key = config.vlm_api_key.strip()
    model = config.vlm_model.strip()
    if not api_base or not api_key or not model:
        raise ValueError("VLM API 地址、API Key、模型名称不能为空")

    image_url = frame_to_jpeg_data_url(frame_bgr)
    body = {
        "model": model,
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "image_url", "image_url": {"url": image_url}},
                    {"type": "text", "text": config.vlm_prompt},
                ],
            }
        ],
        "max_tokens": max(config.vlm_max_tokens, 16),
        "temperature": min(max(config.vlm_temperature, 0.0), 2.0),
    }
    req = urllib.request.Request(
        f"{api_base}/chat/completions",
        data=json.dumps(body).encode("utf-8"),
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            text = resp.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"HTTP {exc.code}: {detail}") from exc

    parsed = json.loads(text)
    content = parsed["choices"][0]["message"]["content"].strip()
    return parse_detection_content(content)


def parse_detection_content(content: str) -> VlmDetection:
    result = (
        _parse_detection_json(content)
        or _parse_detection_json(_extract_code_block(content) or "")
        or _parse_detection_json(_extract_braced_json(content) or "")
    )
    if result is None:
        lower = content.lower()
        result = {
            "has_person": "yes" in lower or "有" in content or "人" in content,
            "detections": [],
        }
    has_person = bool(result.get("has_person", False))
    confidence = 0.7 if has_person else 0.0
    for det in result.get("detections", []) or []:
        try:
            confidence = max(confidence, float(det.get("confidence", 0.0)))
        except (TypeError, ValueError):
            pass
    return VlmDetection(has_person, float(np.clip(confidence, 0.0, 1.0)), content)


def frame_to_jpeg_data_url(frame_bgr: np.ndarray) -> str:
    ok, encoded = cv2.imencode(".jpg", frame_bgr, [int(cv2.IMWRITE_JPEG_QUALITY), 85])
    if not ok:
        raise ValueError("JPEG 编码失败")
    return "data:image/jpeg;base64," + base64.b64encode(encoded.tobytes()).decode("ascii")


def average_light_brightness(gray: np.ndarray, roi_config: Optional[RoiConfig]) -> float:
    rois = (roi_config.light_rois if roi_config else []) or []
    values: list[float] = []
    for roi in rois:
        bounds = roi.bounds(gray.shape[1], gray.shape[0])
        if bounds is None:
            continue
        x1, y1, x2, y2 = bounds
        values.append(float(gray[y1:y2, x1:x2].mean()))
    return float(sum(values) / len(values)) if values else float(gray.mean()) if gray.size else 0.0


def average_color_score(rgb: np.ndarray, gray: np.ndarray, roi_config: Optional[RoiConfig]) -> float:
    if rgb.size == 0:
        return 0.0
    rois = (roi_config.light_rois if roi_config else []) or []
    scores: list[float] = []
    if rois:
        for roi in rois:
            bounds = roi.bounds(gray.shape[1], gray.shape[0])
            if bounds is None:
                continue
            x1, y1, x2, y2 = bounds
            score = color_sum_for_region(rgb[y1:y2, x1:x2])
            if score is not None:
                scores.append(score)
    if scores:
        return float(sum(scores) / len(scores))
    return color_sum_for_region(rgb) or 0.0


def color_sum_for_region(region: np.ndarray) -> Optional[float]:
    if region.size == 0:
        return None
    px = region.astype(np.float32)
    maxc = px.max(axis=2)
    minc = px.min(axis=2)
    luma = (77.0 * px[:, :, 0] + 150.0 * px[:, :, 1] + 29.0 * px[:, :, 2]) / 256.0
    mask = luma >= COLOR_DARK_LUMA_CUTOFF
    if not np.any(mask):
        return 0.0
    chroma = (maxc - minc) / np.maximum(maxc, 1.0)
    weight = np.clip((luma - COLOR_WEIGHT_LUMA_FLOOR) / (255.0 - COLOR_WEIGHT_LUMA_FLOOR), 0.05, 1.0)
    weighted = chroma * weight
    return float(weighted[mask].sum() / weight[mask].sum())


def color_thresholds(roi_config: Optional[RoiConfig]) -> tuple[float, float]:
    if roi_config is None:
        return COLOR_ON_THRESHOLD, COLOR_OFF_THRESHOLD
    if roi_config.light_on_threshold > 0.2 or roi_config.light_off_threshold > 0.2:
        return COLOR_ON_THRESHOLD, COLOR_OFF_THRESHOLD
    on = float(np.clip(roi_config.light_on_threshold, 0.0, 0.2))
    off = float(np.clip(roi_config.light_off_threshold, 0.0, on))
    return on, off


def light_confidence(value: float, light: bool, on_threshold: float, off_threshold: float) -> float:
    distance = max(value - off_threshold, 0.0) if light else max(on_threshold - value, 0.0)
    span = max(abs(on_threshold - off_threshold), 0.01)
    return float(np.clip(distance / span, 0.0, 1.0))


def color_light_confidence(color_score: float, light: bool, on_threshold: float, off_threshold: float) -> float:
    distance = max(color_score - off_threshold, 0.0) if light else max(on_threshold - color_score, 0.0)
    span = max(on_threshold - off_threshold, 0.01)
    return float(np.clip(distance / span, 0.0, 1.0))


def _ema(prev: Optional[float], value: float) -> float:
    return value if prev is None else prev * (1.0 - EMA_ALPHA) + value * EMA_ALPHA


def _to_gray(frame: np.ndarray) -> np.ndarray:
    if frame.ndim == 2:
        return frame
    return cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)


def _to_rgb(frame: np.ndarray) -> np.ndarray:
    if frame.ndim == 2:
        return cv2.cvtColor(frame, cv2.COLOR_GRAY2RGB)
    return cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)


def _parse_detection_json(text: str) -> Optional[dict[str, Any]]:
    if not text.strip():
        return None
    try:
        value = json.loads(text.strip())
    except json.JSONDecodeError:
        return None
    return value if isinstance(value, dict) else None


def _extract_code_block(text: str) -> Optional[str]:
    match = re.search(r"```(?:json)?\s*(.*?)```", text, re.DOTALL | re.IGNORECASE)
    return match.group(1).strip() if match else None


def _extract_braced_json(text: str) -> Optional[str]:
    start = text.find("{")
    end = text.rfind("}")
    return text[start : end + 1] if start >= 0 and end > start else None
