# EcoAlert

视频监控应用：Tauri 桌面 app + 本地测试视频 + 后续可接入的算法处理流水线。

## 目录

| 目录 | 用途 |
| --- | --- |
| [`App/`](./App/) | 产品代码：Tauri 2 桌面端、Vite 前端、Rust 后端 |
| [`Document/`](./Document/) | 产品需求、架构、接口、部署和 ADR |
| [`Video/`](./Video/) | 本地测试视频，全部平铺在根目录 |
| [`Tools/`](./Tools/) | 开发辅助脚本；当前推流器仍是占位 |

## 目录原则

- 顶层只保留产品代码、文档、测试视频、辅助工具四类。
- `Video/` 不再按场景建多级目录，测试时直接引用文件名。
- `Tools/` 只放真正可执行或即将实现的辅助工具，不为想象中的模块预建空目录。
- 设计文档放在 `Document/`，不要把设计说明散落到代码目录。

## 数据流

```
Video/*.mp4 ──► App/webui 视频预览
             └─► Tools/push_streamer（待补 ffmpeg 推流）
                            │
                            ▼
                  App/src-tauri/src/stream/      ← 目标：HLS / RTMP / RTSP 接入
                            │
                            ▼
                  App/src-tauri/src/pipeline/    ← 目标：解码 / 算法 / 报警
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

# 2) 推流器占位命令（当前仅打印参数，ffmpeg 实现待补）
cd Tools
pip install -r requirements.txt
python -m push_streamer.cli --video ../Video/sample_01_meeting.mp4 --loop

# 3) 推流器实现后，在 App「视频管理」里新增
#    http://127.0.0.1:8080/cam1/index.m3u8
```

详细文档看 [`Document/`](./Document/)。
