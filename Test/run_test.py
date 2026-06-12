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

输出
----
- 终端实时打印（摘要模式，每秒一条）
- CSV 文件：Test/output/<video_name>_result.csv

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
from detector import LightDetector, MotionDetector, Rect, SceneAnalyzer, SceneState


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
    return (
        f"[帧 {seq:>6}] "
        f"人={'Y' if state.person else 'N'}  "
        f"灯={'Y' if state.light else 'N'}  "
        f"亮度={state.light_brightness:5.1f}  "
        f"运动={state.motion_score:.3f}  "
        f"置信={state.confidence:.2f}  "
        f"耗时={state.process_ms:.1f}ms  "
        f"→ {label}"
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
#  视频处理
# ============================================================

def process_video(
    video_path: Path,
    analyzer: SceneAnalyzer,
    frame_sample: int = 1,
    skip_seconds: float = 0.0,
    output_dir: Optional[Path] = None,
    verbose: bool = False,
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
                    ])

            frame_idx += 1
    finally:
        cap.release()
        if csv_file:
            csv_file.close()

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
    return True


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
        help="CSV 输出目录（默认 Test/output/）",
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
    )

    # 输出目录
    output_dir = Path(args.output) if args.output else Path(__file__).parent / "output"

    # 处理
    success_count = 0
    fail_count = 0
    for vf in video_files:
        ok = process_video(
            vf, analyzer,
            frame_sample=args.sample,
            skip_seconds=args.skip_seconds,
            output_dir=output_dir,
            verbose=args.verbose,
        )
        if ok:
            success_count += 1
        else:
            fail_count += 1

    print(f"\n处理完成：成功 {success_count} 个，失败 {fail_count} 个")
    if output_dir.exists():
        print(f"CSV 结果已保存到: {output_dir}")


if __name__ == "__main__":
    main()
