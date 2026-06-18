# EcoAlert

视频监控应用：Tauri 桌面 app + 本地测试视频文件导入 + 本地算法处理流水线。

[![License: PolyForm Noncommercial 1.0.0](https://img.shields.io/badge/License-PolyForm%20Noncommercial%201.0.0-red.svg)](./LICENSE)

> Author: **jinsheng.liu1**  ·  License: **PolyForm Noncommercial 1.0.0**（禁止商业使用）  ·  Copyright © 2026

## 目录

| 目录 | 用途 |
| --- | --- |
| [`App/`](./App/) | 产品代码：Tauri 2 桌面端、Vite 前端、Rust 后端 |
| [`Algo/`](./Algo/) | 算法服务与调试代码，包含 YOLO WebSocket 检测服务 |
| [`Document/`](./Document/) | 产品需求、架构、接口、部署和 ADR |
| [`Video/`](./Video/) | 本地测试视频，全部平铺在根目录 |
| [`Tools/`](./Tools/) | 开发辅助脚本；保留可选 ffmpeg HLS 推流器 |

## 目录原则

- 顶层只保留产品代码、文档、测试视频、辅助工具四类。
- `Video/` 不再按场景建多级目录，测试时直接引用文件名。
- `Tools/` 只放真正可执行或即将实现的辅助工具，不为想象中的模块预建空目录。
- 设计文档放在 `Document/`，不要把设计说明散落到代码目录。

## 数据流

```
Video/*.mp4 ──► App 调试页选择文件夹导入为 MP4 视频源
                            │
                            ▼
                  App/webui 循环视频预览
                            │
                            ▼
                  App/src-tauri/src/pipeline/decoder.rs  ← ffmpeg 单帧抽样
                            │
                            ▼
                  App/src-tauri/src/pipeline/    ← ROI 灯光检测 / ROI运动检测 / VLM兜底 / 报警状态机
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

# 2) 测试视频
# 在 Tauri 桌面端登录后进入：基础设置 -> 调试
# 打开“测试视频源”开关并选择文件夹；也可点击“选择测试视频文件夹”重新导入
# App 会递归扫描视频文件并导入为循环播放的 MP4 视频源

# 3) 可选：启用 YOLO 人员检测
cd ..
./scripts/start_yolo.ps1
# 然后在算法设置中启用 YOLO，地址填写 ws://127.0.0.1:8090
```

说明：

- 浏览器预览只验证 UI、视频播放和 mock 数据，不运行 Rust 检测链路。
- Tauri dev/release 才会运行 ffmpeg 抽帧、ROI 灯光检测、报警状态机和通知发送。
- 本地测试不再要求启动推流器；开发者模式保留“测试视频源”开关，开启时选择文件夹自动导入测试视频源，关闭时移除测试源。`Tools/push_streamer` 仅作为需要 HLS 链路时的可选开发工具。
- 当前灯光检测优先使用“开灯彩色、关灯红外黑白”的亮度加权色度分数，并输出明确的开灯 / 关灯状态。
- 当前办公室固定摄像头场景下，人员检测使用 ROI 内帧间运动代理；一帧检测到人后保持 5 分钟。
- 常规模型 5 分钟无人后触发 VLM 兜底；VLM 连续两次确认无人且灯亮才触发报警和通知。
- 开发者模式提供检测历史曲线，实时卡片会分别显示常规模型人员结果、VLM 人员结果、灯光、报警进度、色彩分数、运动分数和耗时。

详细文档看 [`Document/`](./Document/)。
