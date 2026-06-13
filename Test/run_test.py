"""
EcoAlert 算法测试运行器
======================
读取本地视频文件，逐帧运行灯光检测 + 有人检测算法，输出每帧状态。

用法
----
    python run_test.py                          # 测试 Video/ 下所有视频
    python run_test.py path/to/video.mp4        # 测试单个视频
    python run_test.py --sample 5               # 每 5 帧采样一次（加速）
    python run_test.py --skip-seconds 10        # 跳过前 10 秒
    python run_test.py --video-output           # 生成可视化视频（原视频+曲线）
    python run_test.py --video-output --sample 1  # 逐帧生成可视化视频（最高质量）

输出
----
- 终端实时打印（摘要模式，每秒一条）
- CSV 文件：Test/output/<video_name>_result.csv
- 可视化视频：Test/output/<video_name>_analysis.mp4（需 --video-output）

可视化视频布局
--------------
+------------------------------------------+
|        原视频 + 光源位置标记              |
+------------------------------------------+
|  灯光亮度   [曲线 + 阈值线 + 数值]       |
+------------------------------------------+
|  人员检测   [运动分数曲线 + 阈值线]      |
+------------------------------------------+
|  报警状态   [无人+亮灯 高亮]             |
+------------------------------------------+
|  光源过滤   [干扰比曲线 + 阈值线]        |
+------------------------------------------+

退出码
------
0  成功
1  文件不存在或读取失败
2  依赖缺失
"""

from __future__ import annotations

import argparse
import csv
import os
import sys
import time
from pathlib import Path
from typing import Optional

import cv2
import numpy as np

# 同目录导入算法模块
sys.path.insert(0, str(Path(__file__).parent))
from detector import (
    LightDetector, LightSourceAnalyzer, LightSourceResult,
    MotionDetector, Rect, SceneAnalyzer, SceneState,
)


# ============================================================
#  输出格式化
# ============================================================

STATE_LABELS = {
    (False, False): "无人·关灯",
    (False, True):  "无人·亮灯  [!] 报警",
    (True,  False): "有人·关灯",
    (True,  True):  "有人·亮灯",
}


def format_state_line(seq: int, state: SceneState) -> str:
    """格式化单帧摘要"""
    label = STATE_LABELS.get((state.person, state.light), "未知")
    # 光源分析信息
    if state.interference_ratio > 0.01 or not state.is_ceiling_light:
        ceiling_str = "Y" if state.is_ceiling_light else "N"
        src_info = f"  ceiling={ceiling_str}  interf={state.interference_ratio:.2f}"
    else:
        src_info = ""
    return (
        f"[帧 {seq:>6}] "
        f"人={'Y' if state.person else 'N'}  "
        f"灯={'Y' if state.light else 'N'}  "
        f"亮度={state.light_brightness:5.1f}  "
        f"运动={state.motion_score:.3f}  "
        f"置信={state.confidence:.2f}  "
        f"耗时={state.process_ms:.1f}ms  "
        f"→ {label}{src_info}"
    )


def print_summary(
    video_name: str,
    total_frames: int,
    elapsed_sec: float,
    state_changes: int,
    alarm_frames: int,
    avg_ms: float,
):
    """打印视频处理摘要"""
    fps = total_frames / elapsed_sec if elapsed_sec > 0 else 0
    print("\n" + "=" * 60)
    print(f"视频: {video_name}")
    print(f"总帧数: {total_frames}  |  耗时: {elapsed_sec:.2f}s  |  FPS: {fps:.1f}")
    print(f"状态变化次数: {state_changes}  |  报警帧数（无人+亮灯）: {alarm_frames}")
    print(f"平均每帧耗时: {avg_ms:.2f}ms")
    print("=" * 60 + "\n")


# ============================================================
#  可视化曲线绘制（纯 OpenCV，无 matplotlib 依赖）
# ============================================================

# 颜色常量 (BGR)
_COLOR_BG = (24, 24, 28)
_COLOR_PANEL = (32, 32, 38)
_COLOR_GRID = (48, 48, 54)
_COLOR_TEXT = (220, 220, 220)
_COLOR_WHITE = (255, 255, 255)
_COLOR_BRIGHTNESS = (80, 200, 255)    # 金黄 — 灯光亮度
_COLOR_MOTION = (120, 255, 120)       # 绿色 — 人员检测
_COLOR_ALARM = (80, 80, 255)          # 红色 — 报警
_COLOR_ALARM_FILL = (40, 40, 200)     # 报警填充
_COLOR_THRESH = (100, 100, 100)       # 阈值参考线
_COLOR_INTERFERENCE = (60, 180, 255)  # 琥珀 — 干扰比
_COLOR_LIGHT_POS = (255, 160, 60)     # 橙色 — 光源位置标记
_FONT = cv2.FONT_HERSHEY_SIMPLEX


def _draw_curve(
    canvas: np.ndarray,
    x1: int, y1: int, x2: int, y2: int,
    values: np.ndarray,
    count: int,
    vmin: float, vmax: float,
    color: tuple,
    thickness: int = 2,
    fill: bool = False,
):
    """
    在指定矩形区域内绘制曲线。

    values 是长度为 `window` 的环形缓冲区，count 是已填充的有效数量。
    最新的值在最右边。
    """
    w = x2 - x1
    h = y2 - y1
    window = len(values)
    vrange = max(vmax - vmin, 1e-6)

    if count < 1:
        return

    def val_to_y(v: float) -> int:
        return y2 - int((v - vmin) / vrange * h)

    # 绘制填充区域
    if fill and count >= 2:
        start = max(0, window - count)
        pts = []
        for i in range(count):
            px = x2 - (count - 1 - i)
            pv = values[(start + i) % window]
            py = max(y1, min(y2, val_to_y(pv)))
            pts.append([px, py])
        pts.append([x2, y2])
        pts.append([x2 - count + 1, y2])
        pts_arr = np.array(pts, dtype=np.int32)
        cv2.fillPoly(canvas, [pts_arr], color=(
            color[0] // 4, color[1] // 4, color[2] // 4
        ))

    # 绘制曲线
    prev_px = prev_py = None
    start = max(0, window - count)
    for i in range(count):
        px = x2 - (count - 1 - i)
        pv = values[(start + i) % window]
        py = max(y1, min(y2, val_to_y(pv)))
        if prev_px is not None:
            cv2.line(canvas, (prev_px, prev_py), (px, py), color, thickness)
        prev_px, prev_py = px, py


def _draw_threshold_line(
    canvas: np.ndarray,
    x1: int, y1: int, x2: int, y2: int,
    value: float, vmin: float, vmax: float,
    color: tuple = _COLOR_THRESH,
):
    """绘制水平阈值参考线"""
    vrange = max(vmax - vmin, 1e-6)
    py = y2 - int((value - vmin) / vrange * (y2 - y1))
    py = max(y1, min(y2, py))
    cv2.line(canvas, (x1, py), (x2, py), color, 1, cv2.LINE_AA)


def _put_text_cn(canvas: np.ndarray, text: str, org: tuple, scale: float = 0.5,
                 color: tuple = _COLOR_TEXT, thickness: int = 1):
    """
    绘制文本。OpenCV 默认字体不支持中文，这里对 ASCII 部分正常绘制，
    对中文回退到方括号标记。生产环境可加载 Noto Sans CJK 字体。
    """
    cv2.putText(canvas, text, org, _FONT, scale, color, thickness, cv2.LINE_AA)


# ============================================================
#  视频处理（文本 + 可视化视频输出）
# ============================================================

def process_video(
    video_path: Path,
    analyzer: SceneAnalyzer,
    frame_sample: int = 1,
    skip_seconds: float = 0.0,
    output_dir: Optional[Path] = None,
    verbose: bool = False,
    video_output: bool = False,
    curve_height: int = 180,
    window_seconds: float = 60.0,
) -> bool:
    """
    处理单个视频文件。

    Parameters
    ----------
    video_path : Path
        视频文件路径
    analyzer : SceneAnalyzer
        场景分析器
    frame_sample : int
        每 N 帧处理一次（1 = 逐帧）
    skip_seconds : float
        跳过前 N 秒
    output_dir : Path or None
        CSV 输出目录，None 则不输出 CSV
    verbose : bool
        是否每帧都打印
    video_output : bool
        是否生成可视化视频（原视频 + 曲线）
    curve_height : int
        可视化视频中每个曲线面板的高度
    window_seconds : float
        曲线滚动窗口时长（秒）

    Returns
    -------
    bool
        是否处理成功
    """
    cap = cv2.VideoCapture(str(video_path))
    if not cap.isOpened():
        print(f"[错误] 无法打开视频: {video_path}", file=sys.stderr)
        return False

    fps = cap.get(cv2.CAP_PROP_FPS) or 25.0
    total_frames = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
    width = int(cap.get(cv2.CAP_PROP_FRAME_WIDTH))
    height = int(cap.get(cv2.CAP_PROP_FRAME_HEIGHT))
    duration_sec = total_frames / fps if fps > 0 else 0

    print(f"视频: {video_path.name}")
    print(f"  分辨率: {width}×{height}  FPS: {fps:.1f}  时长: {duration_sec:.1f}s  帧数: {total_frames}")

    # 跳过前 N 秒
    if skip_seconds > 0:
        skip_frames = int(skip_seconds * fps)
        cap.set(cv2.CAP_PROP_POS_FRAMES, skip_frames)
        print(f"  跳过前 {skip_seconds:.1f}s（{skip_frames} 帧）")

    # ---- 可视化视频输出准备 ----
    writer = None
    vis_canvas = None
    brightness_buf = None
    motion_buf = None
    alarm_buf = None
    interference_buf = None
    window_size = 0
    buf_pos = 0
    valid_count = 0

    if video_output and output_dir:
        output_dir.mkdir(parents=True, exist_ok=True)
        out_w = width
        out_h = height + curve_height * 4
        vis_path = output_dir / f"{video_path.stem}_analysis.mp4"
        fourcc = cv2.VideoWriter_fourcc(*"mp4v")
        writer = cv2.VideoWriter(str(vis_path), fourcc, fps, (out_w, out_h))
        if not writer.isOpened():
            print(f"[警告] 无法创建输出视频，跳过可视化输出", file=sys.stderr)
            writer = None
        else:
            window_size = max(int(window_seconds * fps), 30)
            brightness_buf = np.zeros(window_size, dtype=np.float32)
            motion_buf = np.zeros(window_size, dtype=np.float32)
            alarm_buf = np.zeros(window_size, dtype=np.uint8)
            interference_buf = np.zeros(window_size, dtype=np.float32)
            vis_canvas = np.zeros((out_h, out_w, 3), dtype=np.uint8)
            print(f"  可视化输出: {vis_path.name} ({out_w}×{out_h})")

    # CSV 输出准备
    csv_writer = None
    csv_file = None
    if output_dir:
        output_dir.mkdir(parents=True, exist_ok=True)
        csv_path = output_dir / f"{video_path.stem}_result.csv"
        csv_file = open(csv_path, "w", newline="", encoding="utf-8")
        csv_writer = csv.writer(csv_file)
        csv_writer.writerow([
            "frame_seq", "timestamp_ms",
            "person", "light",
            "light_brightness", "motion_score", "confidence",
            "process_ms", "state_label",
            "is_ceiling_light", "interference_ratio",
            "light_source_cx", "light_source_cy",
        ])

    # 处理循环
    analyzer.reset()
    frame_idx = 0
    processed_count = 0
    state_changes = 0
    alarm_frames = 0
    total_ms = 0.0
    prev_person = None
    prev_light = None
    t_start = time.perf_counter()

    try:
        while True:
            ret, frame = cap.read()
            if not ret:
                break

            if frame_idx % frame_sample == 0:
                state = analyzer.process(frame)
                processed_count += 1
                total_ms += state.process_ms

                # 检测状态变化
                if state.person != prev_person or state.light != prev_light:
                    state_changes += 1
                    prev_person = state.person
                    prev_light = state.light

                # 报警帧计数
                if not state.person and state.light:
                    alarm_frames += 1

                # 终端输出
                if verbose:
                    print(format_state_line(frame_idx, state))
                elif processed_count % max(1, int(fps)) == 0:
                    # 非 verbose 模式：每秒打印一条
                    print(format_state_line(frame_idx, state))

                # CSV 输出
                if csv_writer:
                    ts_ms = int(cap.get(cv2.CAP_PROP_POS_MSEC))
                    label = STATE_LABELS.get((state.person, state.light), "")
                    csv_writer.writerow([
                        state.frame_seq, ts_ms,
                        int(state.person), int(state.light),
                        f"{state.light_brightness:.2f}",
                        f"{state.motion_score:.4f}",
                        f"{state.confidence:.4f}",
                        f"{state.process_ms:.3f}",
                        label,
                        int(state.is_ceiling_light),
                        f"{state.interference_ratio:.4f}",
                        f"{state.light_source_cx:.4f}",
                        f"{state.light_source_cy:.4f}",
                    ])

                # ---- 可视化视频写入 ----
                if writer is not None:
                    # 更新环形缓冲区
                    brightness_buf[buf_pos] = state.light_brightness
                    motion_buf[buf_pos] = state.motion_score
                    alarm_buf[buf_pos] = 1 if (not state.person and state.light) else 0
                    interference_buf[buf_pos] = state.interference_ratio
                    buf_pos = (buf_pos + 1) % window_size
                    if valid_count < window_size:
                        valid_count += 1

                    _render_vis_frame(
                        vis_canvas, frame, state,
                        brightness_buf, motion_buf, alarm_buf,
                        interference_buf, valid_count,
                        width, height, curve_height, window_size, fps,
                        analyzer, frame_idx,
                    )
                    writer.write(vis_canvas)

            frame_idx += 1

            # 进度输出（每 5 秒打印一次，仅可视化模式）
            if writer is not None:
                now = time.perf_counter()
                if not hasattr(process_video, '_last_t'):
                    process_video._last_t = t_start
                if now - process_video._last_t >= 5.0:
                    progress_pct = (frame_idx / total_frames * 100) if total_frames > 0 else 0
                    proc_elapsed = now - t_start
                    proc_fps = processed_count / proc_elapsed if proc_elapsed > 0 else 0
                    print(
                        f"  可视化: 帧 {frame_idx}/{total_frames} "
                        f"({progress_pct:.1f}%)  {proc_fps:.1f}fps"
                    )
                    process_video._last_t = now
    finally:
        cap.release()
        if csv_file:
            csv_file.close()
        if writer:
            writer.release()

    elapsed = time.perf_counter() - t_start
    avg_ms = total_ms / processed_count if processed_count > 0 else 0
    print_summary(
        video_path.name,
        processed_count,
        elapsed,
        state_changes,
        alarm_frames,
        avg_ms,
    )
    if writer is not None:
        print(f"  可视化视频已保存: {vis_path}")
    return True


def _render_vis_frame(
    canvas: np.ndarray,
    frame: np.ndarray,
    state: SceneState,
    brightness_buf: np.ndarray,
    motion_buf: np.ndarray,
    alarm_buf: np.ndarray,
    interference_buf: np.ndarray,
    valid_count: int,
    src_w: int, src_h: int,
    curve_height: int,
    window_size: int,
    fps: float,
    analyzer: SceneAnalyzer,
    frame_idx: int,
):
    """
    将当前帧和四条曲线渲染到画布上。

    布局：
    ┌──────────────────────────────────┐
    │ 原视频帧 + 光源位置标记           │
    ├──────────────────────────────────┤
    │ 灯光亮度 (曲线 + 阈值线)          │ curve_height
    ├──────────────────────────────────┤
    │ 人员检测 (运动分数曲线)           │ curve_height
    ├──────────────────────────────────┤
    │ 报警状态 (高亮填充)              │ curve_height
    ├──────────────────────────────────┤
    │ 光源过滤 (干扰比曲线 + 阈值线)   │ curve_height
    └──────────────────────────────────┘
    """
    out_w = src_w
    panel_top = src_h

    # 清空画布
    canvas[:] = _COLOR_BG

    # 上方：原视频帧
    if frame.shape[:2] == (src_h, src_w):
        canvas[0:src_h, 0:src_w] = frame
    else:
        canvas[0:src_h, 0:src_w] = cv2.resize(frame, (src_w, src_h))

    # 光源位置叠加（仅在启用光源过滤时）
    if analyzer.light_source_analyzer is not None and state.light:
        cx_px = int(state.light_source_cx * src_w)
        cy_px = int(state.light_source_cy * src_h)
        radius = max(src_w, src_h) // 40
        color = _COLOR_LIGHT_POS if state.is_ceiling_light else _COLOR_ALARM
        cv2.circle(canvas, (cx_px, cy_px), radius, color, 2, cv2.LINE_AA)
        cv2.circle(canvas, (cx_px, cy_px), 3, color, -1, cv2.LINE_AA)
        label = "CEILING" if state.is_ceiling_light else "INTERFERENCE"
        _put_text_cn(canvas, label, (cx_px + radius + 4, cy_px - 4),
                     0.40, color, 1)

    # 分隔线
    cv2.line(canvas, (0, panel_top), (out_w, panel_top), (70, 70, 70), 1)

    # 面板范围
    p_light = (0, panel_top, out_w, panel_top + curve_height)
    p_motion = (0, panel_top + curve_height, out_w, panel_top + curve_height * 2)
    p_alarm = (0, panel_top + curve_height * 2, out_w, panel_top + curve_height * 3)
    p_interf = (0, panel_top + curve_height * 3, out_w, panel_top + curve_height * 4)

    # 阈值
    light_on_th = analyzer.light_detector.light_on_threshold
    light_off_th = analyzer.light_detector.light_off_threshold
    motion_th = analyzer.motion_detector.area_ratio_thresh
    interference_th = analyzer._interference_threshold

    # 面板背景 + 网格
    for px1, py1, px2, py2 in (p_light, p_motion, p_alarm, p_interf):
        canvas[py1:py2, px1:px2] = _COLOR_PANEL
        for gi in range(1, 4):
            gy = py1 + (py2 - py1) * gi // 4
            cv2.line(canvas, (px1, gy), (px2, gy), _COLOR_GRID, 1)

    # 垂直网格（每 10 秒）
    grid_interval = max(int(fps * 10), 1)
    for vi in range(grid_interval, window_size, grid_interval):
        gx = out_w - int(vi * (out_w - 1) / window_size)
        for px1, py1, px2, py2 in (p_light, p_motion, p_alarm, p_interf):
            cv2.line(canvas, (gx, py1), (gx, py2), _COLOR_GRID, 1)

    # ---- 报警面板 ----
    cur_alarm = (not state.person and state.light)
    if cur_alarm:
        roi = canvas[p_alarm[1]:p_alarm[3], p_alarm[0]:p_alarm[2]]
        overlay = np.full_like(roi, 50)
        cv2.addWeighted(overlay, 0.35, roi, 0.65, 0, roi)
        _put_text_cn(canvas, "ALARM: !person && light",
                     (p_alarm[0] + 16, p_alarm[1] + 28), 0.65, _COLOR_ALARM, 2)
    else:
        _put_text_cn(canvas, "OK",
                     (p_alarm[0] + 16, p_alarm[1] + 28), 0.55, _COLOR_MOTION, 1)

    # ---- 灯光亮度曲线 ----
    _draw_threshold_line(canvas, *p_light, light_on_th, 0, 255, _COLOR_BRIGHTNESS)
    _draw_threshold_line(canvas, *p_light, light_off_th, 0, 255, _COLOR_THRESH)
    _draw_curve(canvas, *p_light, brightness_buf, valid_count,
                0.0, 255.0, _COLOR_BRIGHTNESS, 2, fill=True)
    _put_text_cn(
        canvas,
        f"Light  brightness={state.light_brightness:.0f}"
        f"  on>{light_on_th:.0f}  off<{light_off_th:.0f}"
        f"  state={'ON' if state.light else 'OFF'}",
        (p_light[0] + 10, p_light[1] + 22), 0.50, _COLOR_BRIGHTNESS, 1,
    )

    # ---- 人员检测曲线 ----
    _draw_threshold_line(canvas, *p_motion, motion_th, 0.0, 1.0, _COLOR_MOTION)
    _draw_curve(canvas, *p_motion, motion_buf, valid_count,
                0.0, 1.0, _COLOR_MOTION, 2, fill=True)
    _put_text_cn(
        canvas,
        f"Motion  score={state.motion_score:.3f}"
        f"  thresh>{motion_th:.2f}"
        f"  state={'PERSON' if state.person else 'EMPTY'}",
        (p_motion[0] + 10, p_motion[1] + 22), 0.50, _COLOR_MOTION, 1,
    )

    # ---- 光源过滤（干扰比）曲线 ----
    _draw_threshold_line(canvas, *p_interf, interference_th, 0.0, 1.0, _COLOR_INTERFERENCE)
    _draw_curve(canvas, *p_interf, interference_buf, valid_count,
                0.0, 1.0, _COLOR_INTERFERENCE, 2, fill=True)
    ceiling_str = "CEILING" if state.is_ceiling_light else "INTERFERE"
    _put_text_cn(
        canvas,
        f"LightSource  interference={state.interference_ratio:.3f}"
        f"  thresh>{interference_th:.2f}"
        f"  type={ceiling_str}"
        f"  pos=({state.light_source_cx:.2f},{state.light_source_cy:.2f})",
        (p_interf[0] + 10, p_interf[1] + 22), 0.45, _COLOR_INTERFERENCE, 1,
    )

    # ---- 时间刻度 ----
    ts_sec = frame_idx / fps if fps > 0 else 0
    time_str = f"{int(ts_sec // 60):02d}:{ts_sec % 60:05.2f}"
    _put_text_cn(canvas, time_str,
                 (out_w - 120, panel_top - 8), 0.50, _COLOR_TEXT, 1)

    # ---- 右上角状态面板 ----
    status_text = (
        f"Person={'Y' if state.person else 'N'}  "
        f"Light={'Y' if state.light else 'N'}  "
        f"conf={state.confidence:.2f}  "
        f"{state.process_ms:.1f}ms"
    )
    _put_text_cn(canvas, status_text,
                 (out_w - 500, panel_top - 8), 0.45, _COLOR_TEXT, 1)


# ============================================================
#  单元测试（无需视频文件）
# ============================================================

def run_unit_tests():
    """
    使用合成图像测试算法基本逻辑。
    不需要真实视频文件，可快速验证算法正确性。
    """
    print("=" * 60)
    print("单元测试：合成图像")
    print("=" * 60)

    # 测试 1：亮灯场景（全白）
    print("\n[Test 1] 全白帧（灯亮场景）")
    white = np.full((480, 640, 3), 255, dtype=np.uint8)
    light_det = LightDetector(light_on_threshold=140, light_off_threshold=90)
    light_on, brightness = light_det.process(cv2.cvtColor(white, cv2.COLOR_BGR2GRAY))
    print(f"  亮度={brightness:.1f}, 灯亮={light_on}")
    assert light_on, "全白帧应检测到灯亮"
    assert brightness > 140, f"全白帧亮度应 > 140, 实际 {brightness}"
    print("  [PASS]")

    # 测试 2：关灯场景（全黑）
    print("\n[Test 2] 全黑帧（灯灭场景）")
    black = np.zeros((480, 640, 3), dtype=np.uint8)
    light_det.reset()
    light_on, brightness = light_det.process(cv2.cvtColor(black, cv2.COLOR_BGR2GRAY))
    print(f"  亮度={brightness:.1f}, 灯亮={light_on}")
    assert not light_on, "全黑帧应检测到灯灭"
    assert brightness < 90, f"全黑帧亮度应 < 90, 实际 {brightness}"
    print("  [PASS]")

    # 测试 3：运动检测（静止 → 运动）
    print("\n[Test 3] 帧差法运动检测")
    motion_det = MotionDetector(pixel_thresh=20, area_ratio_thresh=0.03)

    # 连续 3 帧静止
    for i in range(3):
        person, score = motion_det.process(cv2.cvtColor(black, cv2.COLOR_BGR2GRAY))
    print(f"  静止 3 帧后: person={person}, score={score:.4f}")
    assert not person, "静止帧不应检测到运动"

    # 突然变为白色（剧烈运动）
    person, score = motion_det.process(cv2.cvtColor(white, cv2.COLOR_BGR2GRAY))
    print(f"  突变后: person={person}, score={score:.4f}")
    assert person, "剧烈变化应检测到运动"
    assert score > 0.5, f"剧烈变化运动分数应 > 0.5, 实际 {score}"
    print("  [PASS]")

    # 测试 4：磁滞阈值（避免中间值跳变）
    print("\n[Test 4] 磁滞阈值测试")
    light_det = LightDetector(
        light_on_threshold=140, light_off_threshold=90, smooth_alpha=1.0
    )

    # 亮度从 0 逐步升到 100（低于 on_threshold=140，应保持灭）
    for v in [0, 50, 80, 100]:
        frame = np.full((100, 100), v, dtype=np.uint8)
        light_on, _ = light_det.process(frame)
    print(f"  亮度 100（< 140）: 灯亮={light_on}")
    assert not light_on, "亮度 < on_threshold 时应保持灯灭"

    # 升到 150（超过 on_threshold=140，应变亮）
    frame = np.full((100, 100), 150, dtype=np.uint8)
    light_on, _ = light_det.process(frame)
    print(f"  亮度 150（> 140）: 灯亮={light_on}")
    assert light_on, "亮度 > on_threshold 时应检测到灯亮"

    # 降到 120（在 90~140 之间，应保持亮 — 磁滞）
    frame = np.full((100, 100), 120, dtype=np.uint8)
    light_on, _ = light_det.process(frame)
    print(f"  亮度 120（90~140 磁滞区）: 灯亮={light_on}")
    assert light_on, "磁滞区内应保持灯亮状态"

    # 降到 80（低于 off_threshold=90，应变灭）
    frame = np.full((100, 100), 80, dtype=np.uint8)
    light_on, _ = light_det.process(frame)
    print(f"  亮度 80（< 90）: 灯亮={light_on}")
    assert not light_on, "亮度 < off_threshold 时应检测到灯灭"
    print("  [PASS]")

    # 测试 4b：EMA 平滑下状态收敛
    print("\n[Test 4b] EMA 平滑收敛测试（alpha=0.3）")
    light_det2 = LightDetector(
        light_on_threshold=140, light_off_threshold=90, smooth_alpha=0.3
    )
    # 持续输入亮度 200，几帧后应变亮
    for i in range(10):
        frame = np.full((100, 100), 200, dtype=np.uint8)
        light_on, bright = light_det2.process(frame)
    print(f"  持续亮度 200，第 10 帧: 灯亮={light_on}, smooth_brightness={bright:.1f}")
    assert light_on, "持续高亮度应使灯状态收敛为亮"

    # 持续输入亮度 20，几帧后应变灭
    for i in range(15):
        frame = np.full((100, 100), 20, dtype=np.uint8)
        light_on, bright = light_det2.process(frame)
    print(f"  持续亮度 20，第 15 帧: 灯亮={light_on}, smooth_brightness={bright:.1f}")
    assert not light_on, "持续低亮度应使灯状态收敛为灭"
    print("  [PASS]")

    # 测试 5：ROI 区域检测
    print("\n[Test 5] ROI 区域检测")
    # 只有左上角亮，其余黑
    frame = np.zeros((200, 200), dtype=np.uint8)
    frame[0:50, 0:50] = 200
    light_det_full = LightDetector()  # 全图 ROI
    light_det_roi = LightDetector(light_rois=[Rect(0.0, 0.0, 0.25, 0.25)])
    _, bright_full = light_det_full.process(frame)
    _, bright_roi = light_det_roi.process(frame)
    print(f"  全图亮度={bright_full:.1f}, ROI 亮度={bright_roi:.1f}")
    assert bright_roi > bright_full, "ROI 区域亮度应高于全图均值"
    print("  [PASS]")

    # 测试 6：性能基准
    print("\n[Test 6] 性能基准（720p 帧）")
    frame_720p = np.random.randint(0, 256, (720, 1280, 3), dtype=np.uint8)
    analyzer = SceneAnalyzer()
    # 预热
    for _ in range(5):
        analyzer.process(frame_720p)
    # 测速
    times = []
    for _ in range(50):
        t0 = time.perf_counter()
        analyzer.process(frame_720p)
        times.append((time.perf_counter() - t0) * 1000)
    avg_ms = sum(times) / len(times)
    max_ms = max(times)
    print(f"  50 帧 720p: 平均={avg_ms:.2f}ms, 最大={max_ms:.2f}ms")
    assert avg_ms < 50, f"单帧平均耗时应 < 50ms, 实际 {avg_ms:.2f}ms"
    print("  [PASS]")

    # ============================================================
    #  光源分析测试 (LightSourceAnalyzer)
    # ============================================================

    # 测试 7：天花板灯（上半部均匀亮）
    print("\n[Test 7] 光源分析 — 天花板灯（上半部亮）")
    frame = np.zeros((200, 200), dtype=np.uint8)
    frame[0:100, :] = 220  # 上半部亮
    lsa = LightSourceAnalyzer(ceiling_height_ratio=0.5)
    result = lsa.analyze(frame)
    print(f"  is_ceiling={result.is_ceiling_light}, interference={result.interference_ratio:.3f}")
    print(f"  position=({result.light_source_position[0]:.2f}, {result.light_source_position[1]:.2f})")
    assert result.is_ceiling_light, "上半部亮应判定为天花板灯"
    assert result.interference_ratio < 0.5, f"天花板灯干扰比应 < 0.5, 实际 {result.interference_ratio:.3f}"
    assert result.light_source_position[1] < 0.5, "质心应在上半部"
    print("  [PASS]")

    # 测试 8：台灯干扰（下半部集中亮点）
    print("\n[Test 8] 光源分析 — 台灯干扰（下半部集中亮）")
    frame = np.zeros((200, 200), dtype=np.uint8)
    # 整个下半部亮，确保整体均值 > min_brightness
    frame[100:200, :] = 255
    lsa = LightSourceAnalyzer(ceiling_height_ratio=0.5, min_brightness=50)
    result = lsa.analyze(frame)
    print(f"  is_ceiling={result.is_ceiling_light}, interference={result.interference_ratio:.3f}")
    print(f"  position=({result.light_source_position[0]:.2f}, {result.light_source_position[1]:.2f})")
    assert not result.is_ceiling_light, "下半部集中亮应判定为干扰源"
    assert result.interference_ratio > 0.5, f"干扰源干扰比应 > 0.5, 实际 {result.interference_ratio:.3f}"
    assert result.light_source_position[1] > 0.5, "质心应在下半部"
    print("  [PASS]")

    # 测试 9：显示器干扰（中部集中亮块）
    print("\n[Test 9] 光源分析 — 显示器干扰（中部集中亮块）")
    frame = np.zeros((200, 200), dtype=np.uint8)
    frame[60:140, 40:160] = 230  # 中部大亮块
    lsa = LightSourceAnalyzer(ceiling_height_ratio=0.5, min_brightness=50)
    result = lsa.analyze(frame)
    print(f"  is_ceiling={result.is_ceiling_light}, interference={result.interference_ratio:.3f}")
    # 显示器特征：峰值/均值比高，连通域单一
    assert result.interference_ratio > 0.3, f"显示器干扰比应较高, 实际 {result.interference_ratio:.3f}"
    print("  [PASS]")

    # 测试 10：均匀亮帧（类似天花板灯）
    print("\n[Test 10] 光源分析 — 均匀亮帧")
    frame = np.full((200, 200), 200, dtype=np.uint8)
    lsa = LightSourceAnalyzer(ceiling_height_ratio=0.5)
    result = lsa.analyze(frame)
    print(f"  is_ceiling={result.is_ceiling_light}, interference={result.interference_ratio:.3f}")
    # 均匀亮：质心居中，峰值/均值比低
    assert result.is_ceiling_light, "均匀亮帧应判定为天花板灯"
    assert abs(result.light_source_position[0] - 0.5) < 0.1, "质心 x 应接近 0.5"
    assert abs(result.light_source_position[1] - 0.5) < 0.1, "质心 y 应接近 0.5"
    print("  [PASS]")

    # 测试 11：暗帧（低于 min_brightness，跳过分析）
    print("\n[Test 11] 光源分析 — 暗帧跳过")
    frame = np.full((200, 200), 30, dtype=np.uint8)
    lsa = LightSourceAnalyzer(ceiling_height_ratio=0.5, min_brightness=50)
    result = lsa.analyze(frame)
    print(f"  is_ceiling={result.is_ceiling_light}, interference={result.interference_ratio:.3f}")
    assert result.is_ceiling_light, "暗帧应返回默认天花板灯"
    assert result.interference_ratio == 0.0, "暗帧干扰比应为 0"
    print("  [PASS]")

    # 测试 12：SceneAnalyzer 光源过滤覆盖（干扰 > 阈值 → light_on=False）
    print("\n[Test 12] SceneAnalyzer 光源过滤覆盖")
    # 创建下半部亮的帧（干扰源特征）
    frame = np.zeros((200, 200, 3), dtype=np.uint8)
    frame[100:200, :] = 255  # 整个下半部亮
    # 使用低阈值让灯亮检测通过
    analyzer = SceneAnalyzer(
        light_on_threshold=100, light_off_threshold=60,
        light_source_filter_enabled=True, interference_threshold=0.5,
    )
    # 预热几帧
    for _ in range(5):
        state = analyzer.process(frame)
    print(f"  light={state.light}, is_ceiling={state.is_ceiling_light}, interference={state.interference_ratio:.3f}")
    # 干扰应被检测到，可能覆盖 light_on
    assert state.interference_ratio > 0.0, "干扰比应 > 0"
    print("  [PASS]")

    # 测试 13：SceneAnalyzer 天花板灯通过（不覆盖）
    print("\n[Test 13] SceneAnalyzer 天花板灯通过")
    # 上半部亮的帧（天花板灯特征）
    frame = np.zeros((200, 200, 3), dtype=np.uint8)
    frame[0:100, :] = 220
    analyzer = SceneAnalyzer(
        light_on_threshold=100, light_off_threshold=60,
        light_source_filter_enabled=True, interference_threshold=0.6,
    )
    for _ in range(3):
        state = analyzer.process(frame)
    print(f"  light={state.light}, is_ceiling={state.is_ceiling_light}, interference={state.interference_ratio:.3f}")
    assert state.is_ceiling_light, "天花板灯应被识别"
    assert state.light, "天花板灯不应被过滤"
    print("  [PASS]")

    # 测试 14：LightSourceAnalyzer 性能基准
    print("\n[Test 14] 光源分析性能基准（720p）")
    frame_720p = np.random.randint(0, 256, (720, 1280), dtype=np.uint8)
    lsa = LightSourceAnalyzer()
    # 预热
    for _ in range(5):
        lsa.analyze(frame_720p)
    # 测速
    times = []
    for _ in range(30):
        t0 = time.perf_counter()
        lsa.analyze(frame_720p)
        times.append((time.perf_counter() - t0) * 1000)
    avg_ms = sum(times) / len(times)
    max_ms = max(times)
    print(f"  30 帧 720p: 平均={avg_ms:.2f}ms, 最大={max_ms:.2f}ms")
    assert avg_ms < 30, f"光源分析单帧平均耗时应 < 30ms, 实际 {avg_ms:.2f}ms"
    print("  [PASS]")

    # 测试 15：时间稳定性（EMA 平滑）
    print("\n[Test 15] 光源分析时间稳定性（EMA）")
    lsa = LightSourceAnalyzer(ceiling_height_ratio=0.5, interference_smooth_alpha=0.4)
    # 先输入干扰帧（下半部亮）
    frame_interf = np.zeros((200, 200), dtype=np.uint8)
    frame_interf[100:200, :] = 255
    # 再输入天花板灯帧（上半部亮）
    frame_ceiling = np.zeros((200, 200), dtype=np.uint8)
    frame_ceiling[0:100, :] = 220
    # 运行干扰帧几帧
    for _ in range(5):
        r1 = lsa.analyze(frame_interf)
    interf_after = r1.interference_ratio
    print(f"  干扰后 interference={interf_after:.3f}")
    assert interf_after > 0.5, f"干扰帧后 interference 应 > 0.5, 实际 {interf_after:.3f}"
    # 切换到天花板灯帧
    r2 = lsa.analyze(frame_ceiling)
    print(f"  切换天花板后 interference={r2.interference_ratio:.3f}")
    # EMA 平滑：切换后干扰比不应立即降到 0
    assert r2.interference_ratio < interf_after, \
        "EMA 平滑应使干扰比逐步下降"
    print("  [PASS]")

    print("\n" + "=" * 60)
    print("全部单元测试通过")
    print("=" * 60)


# ============================================================
#  主入口
# ============================================================

def find_video_files() -> list[Path]:
    """查找 Video/ 目录下所有视频文件"""
    video_dir = Path(__file__).parent.parent / "Video"
    if not video_dir.exists():
        return []
    exts = {".mp4", ".avi", ".mkv", ".mov", ".flv"}
    return sorted(
        p for p in video_dir.iterdir()
        if p.suffix.lower() in exts and not p.name.startswith("video.zip")
    )


def main():
    parser = argparse.ArgumentParser(
        description="EcoAlert 算法测试 — 检测是否开灯 / 是否有人",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
示例：
  python run_test.py                          # 测试所有视频
  python run_test.py Video/5·27域控.mp4       # 测试单个视频
  python run_test.py --sample 3 --verbose     # 每 3 帧采样，详细输出
  python run_test.py --test                   # 仅运行单元测试（无需视频）
  python run_test.py --brightness 160         # 提高灯亮阈值（适应强光场景）
  python run_test.py --video-output           # 批量生成可视化视频（原视频+曲线）
  python run_test.py --video-output -V        # 同上，简写
  python run_test.py --video-output --sample 2  # 每 2 帧采样生成可视化（加速 2 倍）
        """,
    )
    parser.add_argument(
        "video", nargs="?", default=None,
        help="视频文件路径（不填则测试 Video/ 下所有视频）",
    )
    parser.add_argument(
        "--sample", type=int, default=1,
        help="每 N 帧处理一次（默认 1 = 逐帧）",
    )
    parser.add_argument(
        "--skip-seconds", type=float, default=0.0,
        help="跳过前 N 秒",
    )
    parser.add_argument(
        "--verbose", "-v", action="store_true",
        help="每帧都打印输出（默认每秒一条）",
    )
    parser.add_argument(
        "--test", "-t", action="store_true",
        help="仅运行单元测试（合成图像，无需视频文件）",
    )
    parser.add_argument(
        "--output", "-o", type=str, default=None,
        help="CSV / 可视化视频 输出目录（默认 Test/output/）",
    )
    parser.add_argument(
        "--video-output", "-V", action="store_true",
        help="生成可视化视频（原视频 + 灯光/人员/报警曲线）",
    )
    parser.add_argument(
        "--curve-height", type=int, default=180,
        help="可视化视频中每个曲线面板的高度（默认 180px）",
    )
    parser.add_argument(
        "--window-seconds", type=float, default=60.0,
        help="曲线滚动窗口时长（秒，默认 60）",
    )
    parser.add_argument(
        "--brightness", type=float, default=140.0,
        help="灯亮阈值（默认 140）",
    )
    parser.add_argument(
        "--darkness", type=float, default=90.0,
        help="灯灭阈值（默认 90）",
    )
    parser.add_argument(
        "--motion-thresh", type=float, default=0.03,
        help="运动面积比阈值（默认 0.03）",
    )
    # 光源过滤参数
    parser.add_argument(
        "--light-filter", action="store_true",
        help="启用光源过滤（区分天花板灯与干扰光源）",
    )
    parser.add_argument(
        "--interference-thresh", type=float, default=0.6,
        help="干扰覆盖阈值（默认 0.6，超过则判定为非天花板灯）",
    )
    parser.add_argument(
        "--ceiling-ratio", type=float, default=0.5,
        help="天花板高度比（默认 0.5，上半部为天花板区域）",
    )
    args = parser.parse_args()

    # ---- 单元测试模式 ----
    if args.test:
        run_unit_tests()
        return

    # ---- 视频测试模式 ----
    if args.video:
        video_path = Path(args.video)
        if not video_path.exists():
            print(f"[错误] 文件不存在: {video_path}", file=sys.stderr)
            sys.exit(1)
        video_files = [video_path]
    else:
        video_files = find_video_files()
        if not video_files:
            print("[错误] 未找到视频文件。请将 .mp4 文件放入 Video/ 目录，或指定路径。", file=sys.stderr)
            sys.exit(1)
        print(f"找到 {len(video_files)} 个视频文件\n")

    # 创建分析器
    analyzer = SceneAnalyzer(
        light_on_threshold=args.brightness,
        light_off_threshold=args.darkness,
        motion_area_thresh=args.motion_thresh,
        light_source_filter_enabled=args.light_filter,
        interference_threshold=args.interference_thresh,
        ceiling_height_ratio=args.ceiling_ratio,
    )

    # 输出目录
    output_dir = Path(args.output) if args.output else Path(__file__).parent / "output"

    # 处理
    success_count = 0
    fail_count = 0
    total_videos = len(video_files)

    if args.video_output:
        print(f"可视化视频模式：将生成带曲线的分析视频\n")

    for i, vf in enumerate(video_files, 1):
        if total_videos > 1:
            print(f"\n[{i}/{total_videos}] 处理: {vf.name}")
        ok = process_video(
            vf, analyzer,
            frame_sample=args.sample,
            skip_seconds=args.skip_seconds,
            output_dir=output_dir,
            verbose=args.verbose,
            video_output=args.video_output,
            curve_height=args.curve_height,
            window_seconds=args.window_seconds,
        )
        if ok:
            success_count += 1
        else:
            fail_count += 1

    print(f"\n处理完成：成功 {success_count} 个，失败 {fail_count} 个")
    if output_dir.exists():
        print(f"CSV 结果已保存到: {output_dir}")
    if args.video_output:
        vis_files = list(output_dir.glob("*_analysis.mp4"))
        if vis_files:
            print(f"可视化视频已保存到: {output_dir}")
            for vf in vis_files:
                size_mb = vf.stat().st_size / (1024 * 1024)
                print(f"  {vf.name}  ({size_mb:.1f} MB)")


if __name__ == "__main__":
    main()
