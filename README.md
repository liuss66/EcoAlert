# EcoAlert

视频监控应用：Tauri 桌面 app + 本地测试视频 + ffmpeg HLS 推流器 + 本地算法处理流水线。

[![License: PolyForm Noncommercial 1.0.0](https://img.shields.io/badge/License-PolyForm%20Noncommercial%201.0.0-red.svg)](./LICENSE)

> Author: **jinsheng.liu1**  ·  License: **PolyForm Noncommercial 1.0.0**（禁止商业使用）  ·  Copyright © 2026

## 目录

| 目录 | 用途 |
| --- | --- |
| [`App/`](./App/) | 产品代码：Tauri 2 桌面端、Vite 前端、Rust 后端 |
| [`Document/`](./Document/) | 产品需求、架构、接口、部署和 ADR |
| [`Video/`](./Video/) | 本地测试视频，全部平铺在根目录 |
| [`Tools/`](./Tools/) | 开发辅助脚本；当前包含 ffmpeg HLS 推流器 |

## 目录原则

- 顶层只保留产品代码、文档、测试视频、辅助工具四类。
- `Video/` 不再按场景建多级目录，测试时直接引用文件名。
- `Tools/` 只放真正可执行或即将实现的辅助工具，不为想象中的模块预建空目录。
- 设计文档放在 `Document/`，不要把设计说明散落到代码目录。

## 数据流

```
Video/*.mp4 ──► Tools/push_streamer（ffmpeg HLS 推流）
                            │
                            ▼
                  App/webui 视频预览
                            │
                            ▼
                  App/src-tauri/src/pipeline/decoder.rs  ← ffmpeg 单帧抽样
                            │
                            ▼
                  App/src-tauri/src/pipeline/    ← ROI 灯光检测 / 报警状态机
                            │
                            ▼
                  App/webui                      ← UI 展示
```

## 快速开始

```bash
# 1) 应用（前端 + 桌面）
cd App
npm install
npm run dev                # 浏览器预览
npm run tauri:dev          # Tauri 桌面（需要 Rust）

# 2) 推流器（把 Video/*.mp4 转为本地 HLS）
cd Tools
pip install -r requirements.txt
python -m push_streamer.cli --config config.example.yaml

# 3) App 默认源已对应 http://127.0.0.1:8080/cam-N/index.m3u8
```

说明：

- 浏览器预览只验证 UI、视频播放和 mock 数据，不运行 Rust 检测链路。
- Tauri dev/release 才会运行 ffmpeg 抽帧、ROI 灯光检测、报警状态机和通知发送。
- release 包播放本地 HLS 依赖 `tauri.conf.json` 中的本地 `127.0.0.1 / localhost` CSP 白名单，以及针对 WebView2 私网请求限制的 feature 配置。
- 当前灯光检测可用于联调，优先使用“开灯彩色、关灯红外黑白”的亮度加权色度分数，并输出明确的开灯 / 关灯状态；人员存在仍是帧差运动代理，不是真实人形识别，实时卡片会显示开关状态、色彩分数、运动分数和耗时用于排查。

详细文档看 [`Document/`](./Document/)。
