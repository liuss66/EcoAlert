# EcoAlert

EcoAlert 是一个本地运行的视频监控与告警桌面应用，面向办公室、机房、实验室等固定摄像头场景。项目使用 Tauri 2 + Vite 构建桌面端，Rust 后端负责视频抽帧、算法调度、状态持久化和通知发送，前端负责监控大屏、配置和诊断。

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](./LICENSE)

> Author: **jinsheng.liu1** · License: **MIT** · Copyright (c) 2026

## 功能概览

- 多路视频源管理：支持 HLS、MP4、本地视频文件和摄像头 / RTSP 占位接入。
- 本地测试视频导入：Tauri 桌面端可选择文件夹，递归导入 `mp4 / mov / mkv / avi / webm` 文件。
- 本地检测流水线：通过 ffmpeg 抽帧，结合灯光检测、ROI 运动代理和 VLM 兜底判断。
- 报警生命周期：支持疑似、正式报警、确认、恢复和历史记录。
- 通知集成：支持通用 Webhook、飞书、企业微信和 QQ 机器人配置。
- 开发者诊断：提供检测读数、检测历史曲线、ffmpeg 状态和数据初始化工具。

## 项目结构

| 目录 | 用途 |
| --- | --- |
| [`App/`](./App/) | 产品代码：Tauri 2 桌面端、Vite 前端、Rust 后端 |
| [`Algo/`](./Algo/) | 算法服务与调试代码，包含 YOLO WebSocket 检测服务 |
| [`Document/`](./Document/) | 产品需求、架构、接口、部署和 ADR |
| [`Video/`](./Video/) | 本地测试视频，全部平铺在根目录 |
| [`Tools/`](./Tools/) | 开发辅助脚本；保留可选 ffmpeg HLS 推流器 |
| [`scripts/`](./scripts/) | 本地启动、打包和辅助脚本 |
| [`docs/`](./docs/) | 开发计划和补充资料 |

## 目录原则

- 顶层只保留产品代码、文档、测试视频、辅助工具四类。
- `Video/` 不再按场景建多级目录，测试时直接引用文件名。
- `Tools/` 只放真正可执行或即将实现的辅助工具，不为想象中的模块预建空目录。
- 设计文档放在 `Document/`，不要把设计说明散落到代码目录。

## 数据流

```text
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

### 环境要求

- Node.js 18+
- npm
- Rust 1.77+
- ffmpeg 和 ffprobe，并加入 `PATH`；Windows 也可放在应用程序同目录
- 可选：Python 3.10+，用于运行 `Tools/` 和 `Algo/YOLO/` 中的辅助服务

### 启动桌面端

```bash
cd App
npm install
npm run dev                # 浏览器预览
npm run tauri:dev          # Tauri 桌面（需要 Rust）
```

浏览器预览只验证 UI、视频播放和 mock 数据，不运行 Rust 检测链路。Tauri dev/release 才会运行 ffmpeg 抽帧、ROI 灯光检测、报警状态机和通知发送。

### 导入测试视频

1. 启动 Tauri 桌面端并登录。
2. 进入「基础设置 -> 调试」。
3. 打开「测试视频源」开关，选择包含测试视频的文件夹。
4. 系统会递归扫描视频文件，并导入为循环播放的 MP4 视频源。

### 可选：启用 YOLO 人员检测服务

```powershell
cd ..
./scripts/start_yolo.ps1
# 然后在算法设置中启用 YOLO，地址填写 ws://127.0.0.1:8090
```

## 当前检测策略

- 当前灯光检测优先使用“开灯彩色、关灯红外黑白”的亮度加权色度分数，并输出明确的开灯 / 关灯状态。
- 当前办公室固定摄像头场景下，人员检测使用 ROI 内帧间运动代理；一帧检测到人后保持 5 分钟。
- 常规模型 5 分钟无人后触发 VLM 兜底；VLM 连续两次确认无人且灯亮才触发报警和通知。
- 开发者模式提供检测历史曲线，实时卡片会分别显示常规模型人员结果、VLM 人员结果、灯光、报警进度、色彩分数、运动分数和耗时。
- 本地测试不再要求启动推流器；`Tools/push_streamer` 仅作为需要验证 HLS 链路时的可选开发工具。

## 常用命令

```bash
cd App
npm run dev
npm run build
npm run tauri:dev
npm run tauri:build
```

```powershell
# YOLO 服务健康检查
Invoke-RestMethod http://localhost:8090/health
```

## 文档入口

- [`Document/requirements.md`](./Document/requirements.md)：需求、范围和验收标准。
- [`Document/architecture.md`](./Document/architecture.md)：总体架构、模块边界和运行时数据流。
- [`Document/api.md`](./Document/api.md)：Tauri commands、事件和通知契约。
- [`Document/deployment.md`](./Document/deployment.md)：开发、打包、升级和发布检查清单。
- [`Document/user-guide.md`](./Document/user-guide.md)：用户操作流程。
- [`Document/decisions/`](./Document/decisions/)：关键架构决策记录。
- [`Document/changelog/`](./Document/changelog/)：按日期维护的变更日志。

## 贡献约定

- 修改功能前先查看 `Document/` 中的需求、架构和 ADR。
- 用户可见行为变化需要同步更新 `Document/changelog/`。
- 新增配置文件需要包含 `schema_version`，并在升级时考虑迁移和备份。
- 前端字段使用 `camelCase`，后端存储继续兼容已有 `snake_case`。
- 不提交模型权重、测试视频、密钥、真实通知地址和本地应用数据目录。

## 开源协议

本项目采用 [MIT License](./LICENSE) 开源。第三方依赖仍遵循其各自许可证。
