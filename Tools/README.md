# Tools

开发辅助工具目录。包含基于 ffmpeg 的 HLS 推流器，用于将 Video 目录下的视频文件循环推流供 App 端播放。

## 子目录

- `push_streamer/` — HLS 推流器（ffmpeg 子进程管理 + aiohttp 静态服务）
- `config.example.yaml` — 推流器配置示例（8 路视频）
- `scenario/` — 场景配置（多路并发、网络抖动等）

## 前置依赖

- **ffmpeg**：推流器依赖 ffmpeg 做视频转码和 HLS 分片
  - Windows: `winget install ffmpeg` 或从 https://ffmpeg.org/download.html 下载
  - macOS: `brew install ffmpeg`
  - Linux: `sudo apt install ffmpeg`

## 快速开始

```bash
cd Tools
pip install -r requirements.txt

# 方式一：自动扫描 Video 目录下所有 .mp4（快速验证）
python -m push_streamer.cli --auto-scan

# 方式二：使用配置文件（推荐用于 App 联调，端点和页面默认源固定）
python -m push_streamer.cli --config config.example.yaml

# 方式三：单路推流
python -m push_streamer.cli --video ../Video/sample.mp4 --port 8080
```

启动后，HLS 流地址为 `http://127.0.0.1:8080/{name}/index.m3u8`。
App 端默认视频源使用 `config.example.yaml` 的 `cam-1` ~ `cam-8` 映射；联调页面时优先使用配置文件启动。
推流器默认会对输入视频做无限循环，并输出 live HLS 播放列表，适合长时间页面测试。

## HLS 端点命名规则

推流器和 App 端使用统一的 `cam-N` 命名：

| HLS 端点 | 视频文件 | App 源名称 | 分组 |
|----------|---------|-----------|------|
| cam-1 | 4·24域控.mp4 | 4·24 域控 | 域控测试视频 |
| cam-5 | 5·27域控.mp4 | 5·27 域控 | 域控测试视频 |
| cam-6 | 5·28域控.mp4 | 5·28 域控 | 域控测试视频 |
| cam-7 | 5·7域控.mp4 | 5·7 域控 | 域控测试视频 |
| cam-2 | 4·24底盘.mp4 | 4·24 底盘 | 底盘测试视频 |
| cam-4 | 5·15底盘.mp4 | 5·15 底盘 | 底盘测试视频 |
| cam-8 | 5·7底盘.mp4 | 5·7 底盘 | 底盘测试视频 |
| cam-3 | 5·14硬件.mp4 | 5·14 硬件 | 硬件测试视频 |

App 端 `mockDefaultSources()` 中的 URL 已配置为对应的本地 HLS 地址。
