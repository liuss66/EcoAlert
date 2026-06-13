"""推流器命令行入口。

用法:
    python -m push_streamer.cli --config config.example.yaml
    python -m push_streamer.cli --auto-scan
    python -m push_streamer.cli --video ../Video/xxx.mp4
"""
import argparse
import asyncio
import sys
from pathlib import Path

from .push import (
    auto_scan_videos,
    load_config,
    run,
    setup_logging,
    CameraConfig,
)


def main():
    parser = argparse.ArgumentParser(
        description="EcoAlert 推流器 —— ffmpeg 循环推 HLS",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""\
示例:
  # 使用配置文件（多路推流）
  python -m push_streamer.cli --config config.example.yaml

  # 自动扫描 Video 目录
  python -m push_streamer.cli --auto-scan

  # 单路推流
  python -m push_streamer.cli --video ../Video/sample.mp4
""",
    )
    parser.add_argument("--config", help="YAML 配置文件路径（多路推流）")
    parser.add_argument(
        "--auto-scan",
        action="store_true",
        help="自动扫描 ../Video 目录下所有 .mp4 文件",
    )
    parser.add_argument("--video", help="单个视频文件路径")
    parser.add_argument(
        "--port", type=int, default=8080, help="HLS HTTP 服务端口（默认 8080）"
    )
    parser.add_argument("--loop", action="store_true", default=True, help="循环播放（默认开启）")
    args = parser.parse_args()

    setup_logging()
    import logging

    logger = logging.getLogger("push_streamer")

    http_port = args.port
    cameras = []

    if args.config:
        cameras, http_port = load_config(Path(args.config))
    elif args.video:
        video = Path(args.video)
        if not video.is_file():
            logger.error("视频文件不存在: %s", video)
            sys.exit(1)
        name = video.stem.lower().replace(" ", "-")
        cameras = [
            CameraConfig(
                name=name,
                source=video.resolve(),
                loop=args.loop,
            )
        ]
    elif args.auto_scan:
        video_dir = Path(__file__).parent.parent.parent / "Video"
        cameras = auto_scan_videos(video_dir)
    else:
        # 默认：自动扫描
        video_dir = Path(__file__).parent.parent.parent / "Video"
        logger.info("未指定参数，自动扫描 Video 目录: %s", video_dir)
        cameras = auto_scan_videos(video_dir)

    if not cameras:
        logger.error("没有找到任何视频源，请检查配置或 Video 目录")
        sys.exit(1)

    logger.info("发现 %d 路视频源", len(cameras))
    try:
        asyncio.run(run(cameras, http_port))
    except KeyboardInterrupt:
        logger.info("推流器已停止")


if __name__ == "__main__":
    main()
