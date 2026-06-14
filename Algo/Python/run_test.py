"""
EcoAlert App algorithm test runner.

The runner uses Test/detector.py, which mirrors the current App pipeline:
color-based light detection, motion-proxy person detection, optional VLM
person fill, alarm hold/recover logic, and alarm resolution policies.
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

sys.path.insert(0, str(Path(__file__).parent))
from detector import (  # noqa: E402
    AlgorithmConfig,
    COLOR_OFF_THRESHOLD,
    COLOR_ON_THRESHOLD,
    MOTION_AREA_THRESHOLD,
    Rect,
    RoiConfig,
    SceneProcessor,
    SceneState,
    parse_detection_content,
    should_recover_alarm,
)


STATE_LABELS = {
    (False, False): "无人·关灯",
    (False, True): "无人·亮灯",
    (True, False): "有人·关灯",
    (True, True): "有人·亮灯",
}

COLOR_BG = (24, 24, 28)
COLOR_PANEL = (32, 32, 38)
COLOR_GRID = (52, 52, 58)
COLOR_TEXT = (225, 225, 225)
COLOR_LIGHT = (80, 210, 255)
COLOR_COLOR = (255, 170, 80)
COLOR_MOTION = (120, 255, 120)
COLOR_ALARM = (70, 70, 255)
FONT = cv2.FONT_HERSHEY_SIMPLEX


def format_state_line(frame_idx: int, state: SceneState, alarm_status: str, vlm_reason: Optional[str]) -> str:
    label = STATE_LABELS.get((state.person, state.light), "未知")
    vlm = f"  vlm={vlm_reason}" if vlm_reason else ""
    return (
        f"[帧 {frame_idx:>6}] "
        f"人={'Y' if state.person else 'N'}({state.person_confidence:.2f})  "
        f"灯={'Y' if state.light else 'N'}({state.light_confidence:.2f})  "
        f"亮度={state.light_brightness:5.1f}  "
        f"色彩={state.color_score:.3f}  "
        f"运动={state.motion_score:.3f}  "
        f"src={state.source}  alarm={alarm_status}  "
        f"耗时={state.process_ms:.1f}ms  -> {label}{vlm}"
    )


def process_video(
    video_path: Path,
    processor: SceneProcessor,
    frame_sample: int,
    skip_seconds: float,
    max_frames: Optional[int],
    algorithm_width: int,
    algorithm_height: int,
    output_dir: Optional[Path],
    verbose: bool,
    video_output: bool,
    curve_height: int,
    window_seconds: float,
) -> bool:
    cap = cv2.VideoCapture(str(video_path))
    if not cap.isOpened():
        print(f"[错误] 无法打开视频: {video_path}", file=sys.stderr)
        return False

    fps = cap.get(cv2.CAP_PROP_FPS) or 25.0
    total_frames = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
    width = int(cap.get(cv2.CAP_PROP_FRAME_WIDTH))
    height = int(cap.get(cv2.CAP_PROP_FRAME_HEIGHT))
    print(f"视频: {video_path.name}")
    print(f"  分辨率: {width}x{height}  FPS: {fps:.1f}  帧数: {total_frames}")

    if skip_seconds > 0:
        skip_frames = int(skip_seconds * fps)
        cap.set(cv2.CAP_PROP_POS_FRAMES, skip_frames)
        print(f"  跳过前 {skip_seconds:.1f}s ({skip_frames} 帧)")

    csv_file = None
    csv_writer = None
    if output_dir:
        output_dir.mkdir(parents=True, exist_ok=True)
        csv_file = open(output_dir / f"{video_path.stem}_result.csv", "w", newline="", encoding="utf-8")
        csv_writer = csv.writer(csv_file)
        csv_writer.writerow(
            [
                "frame_seq",
                "timestamp_ms",
                "person",
                "light",
                "alarm",
                "alarm_status",
                "person_confidence",
                "light_confidence",
                "confidence",
                "source",
                "reason",
                "light_brightness",
                "color_score",
                "motion_score",
                "process_ms",
                "vlm_reason",
                "vlm_error",
            ]
        )

    writer = None
    vis_canvas = None
    window_size = max(int(window_seconds * fps), 30)
    buffers = {
        "brightness": np.zeros(window_size, dtype=np.float32),
        "color": np.zeros(window_size, dtype=np.float32),
        "motion": np.zeros(window_size, dtype=np.float32),
        "alarm": np.zeros(window_size, dtype=np.float32),
    }
    buf_pos = 0
    valid_count = 0
    if video_output and output_dir:
        out_h = height + curve_height * 4
        out_path = output_dir / f"{video_path.stem}_analysis.mp4"
        writer = cv2.VideoWriter(str(out_path), cv2.VideoWriter_fourcc(*"mp4v"), fps, (width, out_h))
        if writer.isOpened():
            vis_canvas = np.zeros((out_h, width, 3), dtype=np.uint8)
            print(f"  可视化输出: {out_path.name}")
        else:
            writer = None
            print("[警告] 无法创建可视化视频", file=sys.stderr)

    processor.reset()
    frame_idx = 0
    processed_count = 0
    state_changes = 0
    alarm_frames = 0
    active_alarm_frames = 0
    total_ms = 0.0
    prev_state: Optional[tuple[bool, bool]] = None
    started = time.perf_counter()

    try:
        while True:
            if max_frames is not None and processed_count >= max_frames:
                break
            ret, frame = cap.read()
            if not ret:
                break
            if frame_idx % frame_sample != 0:
                frame_idx += 1
                continue

            timestamp_ms = int(cap.get(cv2.CAP_PROP_POS_MSEC))
            algo_frame = cv2.resize(frame, (algorithm_width, algorithm_height), interpolation=cv2.INTER_AREA)
            result = processor.process(algo_frame, now_ms=timestamp_ms)
            state = result.state
            processed_count += 1
            total_ms += state.process_ms
            state_key = (state.person, state.light)
            if state_key != prev_state:
                state_changes += 1
                prev_state = state_key
            if result.raw_alarm:
                alarm_frames += 1
            if result.alarm_status == "alarm_active":
                active_alarm_frames += 1

            if verbose or processed_count % max(1, int(fps / max(frame_sample, 1))) == 0:
                print(format_state_line(frame_idx, state, result.alarm_status, result.vlm_reason))
                if result.vlm_error:
                    print(f"  [VLM] {result.vlm_error}")

            if csv_writer:
                csv_writer.writerow(
                    [
                        state.frame_seq,
                        timestamp_ms,
                        int(state.person),
                        int(state.light),
                        int(result.raw_alarm),
                        result.alarm_status,
                        f"{state.person_confidence:.4f}",
                        f"{state.light_confidence:.4f}",
                        f"{state.confidence:.4f}",
                        state.source,
                        state.reason or "",
                        f"{state.light_brightness:.2f}",
                        f"{state.color_score:.5f}",
                        f"{state.motion_score:.5f}",
                        f"{state.process_ms:.3f}",
                        result.vlm_reason or "",
                        result.vlm_error or "",
                    ]
                )

            if writer is not None and vis_canvas is not None:
                buffers["brightness"][buf_pos] = state.light_brightness
                buffers["color"][buf_pos] = state.color_score
                buffers["motion"][buf_pos] = state.motion_score
                buffers["alarm"][buf_pos] = 1.0 if result.alarm_status == "alarm_active" else 0.0
                buf_pos = (buf_pos + 1) % window_size
                valid_count = min(valid_count + 1, window_size)
                render_vis_frame(
                    vis_canvas,
                    frame,
                    state,
                    result.alarm_status,
                    buffers,
                    valid_count,
                    width,
                    height,
                    curve_height,
                    processor,
                    frame_idx,
                    fps,
                )
                writer.write(vis_canvas)

            frame_idx += 1
    finally:
        cap.release()
        if csv_file:
            csv_file.close()
        if writer:
            writer.release()

    elapsed = time.perf_counter() - started
    avg_ms = total_ms / processed_count if processed_count else 0.0
    proc_fps = processed_count / elapsed if elapsed > 0 else 0.0
    print("\n" + "=" * 60)
    print(f"视频: {video_path.name}")
    print(f"处理帧数: {processed_count}  耗时: {elapsed:.2f}s  处理FPS: {proc_fps:.1f}")
    print(f"状态变化: {state_changes}  疑似报警帧: {alarm_frames}  活跃报警帧: {active_alarm_frames}")
    print(f"平均算法耗时: {avg_ms:.2f}ms")
    print("=" * 60 + "\n")
    return True


def render_vis_frame(
    canvas: np.ndarray,
    frame: np.ndarray,
    state: SceneState,
    alarm_status: str,
    buffers: dict[str, np.ndarray],
    valid_count: int,
    src_w: int,
    src_h: int,
    curve_height: int,
    processor: SceneProcessor,
    frame_idx: int,
    fps: float,
) -> None:
    canvas[:] = COLOR_BG
    canvas[0:src_h, 0:src_w] = frame if frame.shape[:2] == (src_h, src_w) else cv2.resize(frame, (src_w, src_h))
    status = (
        f"Person={'Y' if state.person else 'N'} {state.person_confidence:.2f}  "
        f"Light={'Y' if state.light else 'N'} {state.light_confidence:.2f}  "
        f"Alarm={alarm_status}  src={state.source}"
    )
    cv2.rectangle(canvas, (0, src_h - 34), (src_w, src_h), (0, 0, 0), -1)
    cv2.putText(canvas, status, (14, src_h - 11), FONT, 0.62, COLOR_TEXT, 1, cv2.LINE_AA)

    panels = [
        ("brightness", "brightness", 0.0, 255.0, COLOR_LIGHT, [processor.config.light_threshold * 255.0]),
        ("color", "color_score", 0.0, 0.16, COLOR_COLOR, [COLOR_ON_THRESHOLD, COLOR_OFF_THRESHOLD]),
        ("motion", "motion_score", 0.0, 1.0, COLOR_MOTION, [processor.detector.effective_person_threshold()]),
        ("alarm", "alarm_active", 0.0, 1.0, COLOR_ALARM, [0.5]),
    ]
    for i, (key, label, vmin, vmax, color, thresholds) in enumerate(panels):
        y1 = src_h + i * curve_height
        y2 = y1 + curve_height
        canvas[y1:y2, :] = COLOR_PANEL
        for g in range(1, 4):
            gy = y1 + curve_height * g // 4
            cv2.line(canvas, (0, gy), (src_w, gy), COLOR_GRID, 1)
        for threshold in thresholds:
            draw_threshold(canvas, 0, y1, src_w, y2, threshold, vmin, vmax)
        draw_curve(canvas, 0, y1, src_w, y2, buffers[key], valid_count, vmin, vmax, color)
        cur = float(buffers[key][(valid_count - 1) % len(buffers[key])]) if valid_count else 0.0
        cv2.putText(canvas, f"{label}: {cur:.3f}", (14, y1 + 28), FONT, 0.62, COLOR_TEXT, 1, cv2.LINE_AA)

    sec = frame_idx / max(fps, 1.0)
    cv2.putText(canvas, f"t={sec:7.2f}s", (src_w - 130, src_h - 11), FONT, 0.55, COLOR_TEXT, 1, cv2.LINE_AA)


def draw_curve(
    canvas: np.ndarray,
    x1: int,
    y1: int,
    x2: int,
    y2: int,
    values: np.ndarray,
    count: int,
    vmin: float,
    vmax: float,
    color: tuple[int, int, int],
) -> None:
    if count < 2:
        return
    count = min(count, len(values), x2 - x1)
    span = max(vmax - vmin, 1e-6)
    start = (count - 1) % len(values)
    pts = []
    for i in range(count):
        idx = (start - (count - 1 - i)) % len(values)
        px = x2 - count + i
        py = y2 - int((float(values[idx]) - vmin) / span * (y2 - y1))
        pts.append((px, max(y1, min(y2, py))))
    for p1, p2 in zip(pts, pts[1:]):
        cv2.line(canvas, p1, p2, color, 2, cv2.LINE_AA)


def draw_threshold(
    canvas: np.ndarray,
    x1: int,
    y1: int,
    x2: int,
    y2: int,
    value: float,
    vmin: float,
    vmax: float,
) -> None:
    span = max(vmax - vmin, 1e-6)
    py = y2 - int((value - vmin) / span * (y2 - y1))
    py = max(y1, min(y2, py))
    cv2.line(canvas, (x1, py), (x2, py), (100, 100, 100), 1, cv2.LINE_AA)


def run_unit_tests() -> None:
    print("=" * 60)
    print("单元测试：App 算法镜像")
    print("=" * 60)

    infrared = np.full((120, 160, 3), 128, dtype=np.uint8)
    color_on = np.zeros((120, 160, 3), dtype=np.uint8)
    color_on[:, :] = (40, 120, 220)  # BGR -> RGB [220,120,40]
    processor = SceneProcessor(AlgorithmConfig(alarm_hold_sec=0, alarm_recover_sec=0))
    state = processor.process(infrared, now_ms=0).state
    assert not state.light
    assert state.color_score < COLOR_OFF_THRESHOLD
    for i in range(4):
        state = processor.process(color_on, now_ms=(i + 1) * 1000).state
    assert state.light
    assert state.color_score > COLOR_ON_THRESHOLD
    print("[PASS] RGB 色彩分数开灯检测")

    processor.reset()
    black = np.zeros((120, 160, 3), dtype=np.uint8)
    white = np.full((120, 160, 3), 255, dtype=np.uint8)
    first = processor.process(black, now_ms=0).state
    second = processor.process(white, now_ms=1000).state
    assert not first.person
    assert second.person
    assert second.motion_score > MOTION_AREA_THRESHOLD
    print("[PASS] 运动代理有人检测")

    roi = RoiConfig(light_rois=[Rect(0.25, 0.25, 0.5, 0.5)], light_on_threshold=0.03, light_off_threshold=0.015)
    frame = np.full((100, 100, 3), (110, 120, 160), dtype=np.uint8)
    processor = SceneProcessor(AlgorithmConfig(alarm_hold_sec=0), roi)
    state = processor.process(frame, now_ms=0).state
    assert state.light
    assert "light_by_color" in (state.reason or "")
    print("[PASS] ROI 彩色阈值兼容")

    parsed = parse_detection_content('```json\n{"has_person":true,"detections":[{"confidence":0.88}]}\n```')
    assert parsed.has_person and parsed.confidence == 0.88
    parsed = parse_detection_content('{"has_person":false,"detections":[]}')
    assert not parsed.has_person and parsed.confidence == 0.0
    print("[PASS] VLM 响应解析")

    assert should_recover_alarm(False, False, "light_off")
    assert should_recover_alarm(True, True, "person_present")
    assert should_recover_alarm(True, False, "both")
    assert should_recover_alarm(False, False, "either")
    processor = SceneProcessor(AlgorithmConfig(alarm_hold_sec=10, alarm_recover_sec=5))
    assert processor.alarm_timer.update(True, False, 1_000, 10, 5) is None
    assert processor.alarm_timer.update(True, False, 11_000, 10, 5) is True
    assert processor.alarm_timer.update(False, True, 12_000, 10, 5) is None
    assert processor.alarm_timer.update(False, True, 17_000, 10, 5) is False
    print("[PASS] 报警保持/恢复逻辑")

    print("=" * 60)
    print("全部单元测试通过")
    print("=" * 60)


def find_video_files() -> list[Path]:
    video_dir = Path(__file__).parent.parent / "Video"
    if not video_dir.exists():
        return []
    exts = {".mp4", ".avi", ".mkv", ".mov", ".flv"}
    return sorted(p for p in video_dir.iterdir() if p.suffix.lower() in exts and not p.name.startswith("video.zip"))


def parse_roi(value: Optional[str]) -> RoiConfig:
    if not value:
        return RoiConfig()
    rois = []
    for item in value.split(";"):
        parts = [p.strip() for p in item.split(",")]
        if len(parts) != 4:
            raise ValueError("--light-roi 格式应为 x,y,w,h，可用分号分隔多个 ROI")
        rois.append(Rect(*(float(p) for p in parts)))
    return RoiConfig(light_rois=rois)


def main() -> None:
    parser = argparse.ArgumentParser(description="EcoAlert App 算法测试")
    parser.add_argument("video", nargs="?", default=None, help="视频文件路径；不填则测试 Video/ 下所有视频")
    parser.add_argument("--sample", type=int, default=1, help="每 N 帧处理一次")
    parser.add_argument("--skip-seconds", type=float, default=0.0, help="跳过前 N 秒")
    parser.add_argument("--max-frames", type=int, default=None, help="最多处理 N 帧，便于快速调试")
    parser.add_argument("--algorithm-width", type=int, default=160, help="算法输入宽度，默认与 App 抽帧一致")
    parser.add_argument("--algorithm-height", type=int, default=120, help="算法输入高度，默认与 App 抽帧一致")
    parser.add_argument("--verbose", "-v", action="store_true", help="每帧打印")
    parser.add_argument("--test", "-t", action="store_true", help="运行合成单元测试")
    parser.add_argument("--output", "-o", type=str, default=None, help="输出目录，默认 Test/output")
    parser.add_argument("--video-output", "-V", action="store_true", help="生成可视化视频")
    parser.add_argument("--curve-height", type=int, default=140, help="曲线面板高度")
    parser.add_argument("--window-seconds", type=float, default=60.0, help="曲线窗口秒数")
    parser.add_argument("--person-threshold", type=float, default=0.65, help="App person_threshold")
    parser.add_argument("--light-threshold", type=float, default=0.70, help="无 RGB 时亮度兜底阈值 0..1")
    parser.add_argument("--light-roi", default=None, help="灯光 ROI，格式 x,y,w,h;多个用分号分隔")
    parser.add_argument("--roi-color-on", type=float, default=COLOR_ON_THRESHOLD, help="ROI 色彩开灯阈值")
    parser.add_argument("--roi-color-off", type=float, default=COLOR_OFF_THRESHOLD, help="ROI 色彩关灯阈值")
    parser.add_argument("--alarm-hold-sec", type=int, default=300, help="报警保持秒数")
    parser.add_argument("--alarm-recover-sec", type=int, default=60, help="报警恢复秒数")
    parser.add_argument("--recover-policy", default="either", choices=["light_off", "person_present", "both", "either"])
    parser.add_argument("--vlm", action="store_true", help="启用 VLM 补漏")
    parser.add_argument("--vlm-api-base", default=os.getenv("ECOALERT_VLM_API_BASE", ""))
    parser.add_argument("--vlm-api-key", default=os.getenv("ECOALERT_VLM_API_KEY", ""))
    parser.add_argument("--vlm-model", default=os.getenv("ECOALERT_VLM_MODEL", ""))
    parser.add_argument("--vlm-interval-sec", type=int, default=300)
    parser.add_argument("--vlm-hourly-limit", type=int, default=12)
    parser.add_argument("--vlm-no-skip-when-person", action="store_true", help="有人时也运行 VLM")
    args = parser.parse_args()

    if args.test:
        run_unit_tests()
        return

    roi_config = parse_roi(args.light_roi)
    roi_config.light_on_threshold = args.roi_color_on
    roi_config.light_off_threshold = args.roi_color_off
    config = AlgorithmConfig(
        person_threshold=args.person_threshold,
        light_threshold=args.light_threshold,
        alarm_hold_sec=args.alarm_hold_sec,
        alarm_recover_sec=args.alarm_recover_sec,
        recover_policy=args.recover_policy,
        vlm_enabled=args.vlm,
        vlm_skip_when_person=not args.vlm_no_skip_when_person,
        vlm_interval_sec=args.vlm_interval_sec,
        vlm_hourly_limit=args.vlm_hourly_limit,
        vlm_api_base=args.vlm_api_base,
        vlm_api_key=args.vlm_api_key,
        vlm_model=args.vlm_model,
    )
    processor = SceneProcessor(config, roi_config)

    if args.video:
        video_path = Path(args.video)
        if not video_path.exists():
            print(f"[错误] 文件不存在: {video_path}", file=sys.stderr)
            sys.exit(1)
        video_files = [video_path]
    else:
        video_files = find_video_files()
        if not video_files:
            print("[错误] 未找到视频文件。请将 .mp4 文件放入 Video/，或指定路径。", file=sys.stderr)
            sys.exit(1)

    output_dir = Path(args.output) if args.output else Path(__file__).parent / "output"
    success = 0
    failed = 0
    for video in video_files:
        ok = process_video(
            video,
            processor,
            max(args.sample, 1),
            args.skip_seconds,
            args.max_frames,
            args.algorithm_width,
            args.algorithm_height,
            output_dir,
            args.verbose,
            args.video_output,
            args.curve_height,
            args.window_seconds,
        )
        success += int(ok)
        failed += int(not ok)
    print(f"处理完成：成功 {success} 个，失败 {failed} 个")
    print(f"结果目录: {output_dir}")


if __name__ == "__main__":
    main()
