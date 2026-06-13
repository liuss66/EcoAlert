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

# 方式一：自动扫描 Video 目录下所有 .mp4（推荐）
python -m push_streamer.cli --auto-scan

# 方式二：使用配置文件（精确控制每路参数）
python -m push_streamer.cli --config config.example.yaml

# 方式三：单路推流
python -m push_streamer.cli --video ../Video/sample.mp4 --port 8080
```

启动后，HLS 流地址为 `http://127.0.0.1:8080/{name}/index.m3u8`。

## HLS 端点命名规则

推流器和 App 端使用统一的 `cam-N` 命名：

| HLS 端点 | 视频文件 | App 源名称 | 位置 |
|----------|---------|-----------|------|
| cam-1 | 4·24域控.mp4 | 大堂入口 | A栋 1F |
| cam-2 | 4·24底盘.mp4 | 前台接待区 | A栋 1F |
| cam-3 | 5·14硬件.mp4 | 电梯厅 | A栋 1F |
| cam-4 | 5·15底盘.mp4 | 茶水间 | A栋 2F |
| cam-5 | 5·27域控.mp4 | 生产线 1 | B栋 车间 |
| cam-6 | 5·28域控.mp4 | 原材料仓库 | B栋 仓库 |
| cam-7 | 5·7域控.mp4 | 园区东门 | 园区 周界 |
| cam-8 | 5·7底盘.mp4 | 核心机房 | 核心 机房 |

App 端 `mockDefaultSources()` 中的 URL 已配置为对应的本地 HLS 地址。
复用流的源（生产线 2 / 园区西门 / 停车场 / UPS 配电室）默认禁用。
