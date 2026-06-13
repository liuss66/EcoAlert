"""推流器核心逻辑：ffmpeg 循环推流 + HTTP 服务托管 HLS 分片。

用法：
    python -m push_streamer.cli --config config.example.yaml
    python -m push_streamer.cli --auto-scan          # 自动扫描 ../Video/*.mp4
    python -m push_streamer.cli --video ../Video/xxx.mp4
"""
import asyncio
import logging
import os
import shutil
import signal
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional

import yaml
from aiohttp import web

logger = logging.getLogger("push_streamer")

HLS_CONTENT_TYPES = {
    ".m3u8": "application/vnd.apple.mpegurl",
    ".ts": "video/mp2t",
}


@dataclass
class CameraConfig:
    """单路摄像头推流配置。"""

    name: str  # HLS 端点名 (如 cam-1)
    source: Path  # 视频文件绝对路径
    location: str = ""
    protocol: str = "hls"
    bitrate: int = 1500
    loop: bool = True
    segment_time: int = 2


# ────────────────────────── ffmpeg 进程管理 ──────────────────────────


def spawn_ffmpeg(cam: CameraConfig, hls_dir: Path) -> Optional[subprocess.Popen]:
    """为一路摄像头启动 ffmpeg 子进程（循环推流 → HLS 分片）。"""
    out_sub = hls_dir / cam.name
    out_sub.mkdir(parents=True, exist_ok=True)
    playlist = out_sub / "index.m3u8"

    vf_chain = "setpts=N/FRAME_RATE/TB,setdar=dar=16/9"

    cmd = [
        "ffmpeg",
        "-hide_banner",
        "-loglevel", "warning",
        "-re",  # 按原始帧率读取（限速）
        "-stream_loop", "-1",  # 无限循环输入
        "-i", str(cam.source.resolve()),
        "-vf", vf_chain,
        "-c:v", "libx264",
        "-preset", "veryfast",
        "-b:v", f"{cam.bitrate}k",
        "-maxrate", f"{cam.bitrate * 12 // 10}k",
        "-bufsize", f"{cam.bitrate * 2}k",
        "-g", str(cam.segment_time * 25),  # GOP = 切片时长 × 帧率
        "-c:a", "aac",
        "-b:a", "128k",
        "-f", "hls",
        "-hls_time", str(cam.segment_time),
        "-hls_list_size", "10",
        "-hls_flags", "delete_segments+append_list",
        "-hls_segment_filename", str(out_sub / "seg_%05d.ts"),
        str(playlist),
    ]

    logger.debug("ffmpeg cmd: %s", " ".join(cmd))

    try:
        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            creationflags=subprocess.CREATE_NO_WINDOW
            if sys.platform == "win32"
            else 0,
        )
        logger.info("[✓] %s  视频: %s", cam.name, cam.source.name)
        return proc
    except FileNotFoundError:
        logger.error(
            "找不到 ffmpeg，请先安装: https://ffmpeg.org/download.html"
        )
        return None
    except Exception as exc:
        logger.error("[%s] 启动 ffmpeg 失败: %s", cam.name, exc)
        return None


# ────────────────────────── HLS HTTP 服务 ──────────────────────────


def make_hls_app(hls_dir: Path) -> web.Application:
    """创建 aiohttp Application，托管 HLS 分片（支持 CORS）。"""

    async def handle_hls(request: web.Request) -> web.StreamResponse:
        rel = request.match_info["path"]
        file_path = (hls_dir / rel).resolve()

        # 安全：只允许访问 hls_dir 下的文件
        if not str(file_path).startswith(str(hls_dir.resolve())):
            return web.Response(status=403, text="Forbidden")
        if not file_path.is_file():
            return web.Response(status=404, text="Not found")

        ct = HLS_CONTENT_TYPES.get(file_path.suffix, "application/octet-stream")
        return web.FileResponse(
            file_path,
            headers={
                "Access-Control-Allow-Origin": "*",
                "Access-Control-Allow-Methods": "GET, OPTIONS",
                "Access-Control-Allow-Headers": "*",
                "Cache-Control": "no-cache",
                "Content-Type": ct,
            },
        )

    async def handle_options(request: web.Request) -> web.Response:
        return web.Response(
            headers={
                "Access-Control-Allow-Origin": "*",
                "Access-Control-Allow-Methods": "GET, OPTIONS",
                "Access-Control-Allow-Headers": "*",
            }
        )

    app = web.Application()
    app.router.add_options("/{path:.*}", handle_options)
    app.router.add_get("/{path:.*}", handle_hls)
    return app


async def run_http_server(hls_dir: Path, port: int) -> None:
    """启动 HTTP 服务，阻塞直到取消。"""
    app = make_hls_app(hls_dir)
    runner = web.AppRunner(app, access_log=None)
    await runner.setup()
    site = web.TCPSite(runner, "0.0.0.0", port)
    await site.start()
    logger.info("HLS HTTP 服务已启动 → http://127.0.0.1:%d", port)
    logger.info("示例播放地址: http://127.0.0.1:%d/cam-1/index.m3u8", port)
    # 保持运行
    try:
        while True:
            await asyncio.sleep(3600)
    except asyncio.CancelledError:
        pass
    finally:
        await runner.cleanup()


# ────────────────────────── 配置加载 ──────────────────────────


def load_config(config_path: Path) -> tuple:
    """从 YAML 加载配置，返回 (cameras, http_port)。"""
    with open(config_path, encoding="utf-8") as f:
        cfg = yaml.safe_load(f)

    server = cfg.get("server", {})
    http_port = server.get("http_port", 8080)
    defaults = cfg.get("defaults", {})
    loop = defaults.get("loop", True)
    segment_time = defaults.get("segment_time", 2)

    config_dir = config_path.parent
    cameras: List[CameraConfig] = []

    for entry in cfg.get("cameras", []):
        raw_source = entry["source"]
        source = Path(raw_source)
        if not source.is_absolute():
            source = (config_dir / source).resolve()

        if not source.is_file():
            logger.warning("视频文件不存在，跳过: %s", source)
            continue

        cameras.append(
            CameraConfig(
                name=entry["name"],
                source=source,
                location=entry.get("location", ""),
                protocol=entry.get("protocol", "hls"),
                bitrate=entry.get("bitrate", 1500),
                loop=entry.get("loop", loop),
                segment_time=entry.get("segment_time", segment_time),
            )
        )

    return cameras, http_port


def auto_scan_videos(video_dir: Path) -> List[CameraConfig]:
    """自动扫描 Video 目录下所有 .mp4 文件，生成推流配置。

    命名规则: cam-1, cam-2, ... （按文件名排序）
    """
    if not video_dir.is_dir():
        logger.error("Video 目录不存在: %s", video_dir)
        return []

    videos = sorted(video_dir.glob("*.mp4"))
    if not videos:
        logger.error("Video 目录下没有 mp4 文件: %s", video_dir)
        return []

    cameras = []
    for i, v in enumerate(videos, 1):
        cameras.append(
            CameraConfig(
                name=f"cam-{i}",
                source=v.resolve(),
                location="",
                bitrate=1500,
            )
        )
    return cameras


# ────────────────────────── 主流程 ──────────────────────────


def setup_logging() -> None:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(message)s",
        datefmt="%H:%M:%S",
    )


def print_stream_table(cameras: List[CameraConfig], port: int) -> None:
    """打印推流列表，方便确认命名对应关系。"""
    logger.info("─" * 60)
    logger.info("  %-10s %-20s %s", "HLS 端点", "视频文件", "位置")
    logger.info("─" * 60)
    for cam in cameras:
        url = f"http://127.0.0.1:{port}/{cam.name}/index.m3u8"
        logger.info("  %-10s %-20s %s", cam.name, cam.source.name, cam.location or "-")
    logger.info("─" * 60)


async def run(cameras: List[CameraConfig], http_port: int) -> None:
    """启动所有 ffmpeg 进程 + HTTP 服务。"""
    hls_dir = Path(tempfile.mkdtemp(prefix="ecoalert_hls_"))
    logger.info("HLS 输出目录: %s", hls_dir)

    print_stream_table(cameras, http_port)

    procs: List[subprocess.Popen] = []
    for cam in cameras:
        p = spawn_ffmpeg(cam, hls_dir)
        if p:
            procs.append(p)

    if not procs:
        logger.error("没有成功启动任何推流进程，请检查 ffmpeg 是否已安装")
        shutil.rmtree(hls_dir, ignore_errors=True)
        return

    logger.info("共 %d/%d 路推流已启动", len(procs), len(cameras))

    try:
        await run_http_server(hls_dir, http_port)
    except KeyboardInterrupt:
        logger.info("收到中断信号，正在停止…")
    finally:
        for p in procs:
            try:
                p.terminate()
            except OSError:
                pass
        for p in procs:
            try:
                p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                p.kill()
        shutil.rmtree(hls_dir, ignore_errors=True)
        logger.info("已清理，全部推流已停止")
