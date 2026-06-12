"""推流器命令行入口（占位）。"""
# 真实实现：解析 config + spawn ffmpeg 子进程 + 起 HTTP 服务托管 HLS 分片
import argparse


def main():
    parser = argparse.ArgumentParser(description="EcoAlert 推流模拟器")
    parser.add_argument("--video", help="单个视频文件")
    parser.add_argument("--config", help="YAML 配置文件（多路）")
    parser.add_argument("--output", help="输出地址，如 http://127.0.0.1:8080/cam1/index.m3u8")
    parser.add_argument("--loop", action="store_true", help="循环播放")
    parser.add_argument("--rtmp", action="store_true", help="用 RTMP 推流（默认 HLS）")
    args = parser.parse_args()
    # TODO: 调 ffmpeg
    print(f"[stub] video={args.video} output={args.output} loop={args.loop} rtmp={args.rtmp}")


if __name__ == "__main__":
    main()
