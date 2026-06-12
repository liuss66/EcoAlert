# EcoAlert

视频监控应用：Tauri 桌面 app + 推流模拟器 + 算法处理流水线。

## 目录

| 目录 | 用途 |
| --- | --- |
| [`App/`](./App/) | 应用代码（Tauri 2 + Vite 前端 + Rust 后端 + 视频处理流水线） |
| [`Document/`](./Document/) | 需求 / 架构 / 接口 / 部署文档 |
| [`Video/`](./Video/) | 测试视频（samples / long / benchmark） |
| [`Tools/`](./Tools/) | 推流模拟器，把 `Video/` 里的视频当实时源推给 App |
| `video.zip` | 原始下载包，下载完后解压到 `Video/` |

## 数据流

```
Video/  ──(本地文件)──►  Tools/push_streamer
                              │
                              │  HLS / RTMP
                              ▼
                    App/src-tauri/src/stream/        ← 推流承接
                              │  帧
                              ▼
                    App/src-tauri/src/pipeline/      ← 处理流水线
                              │  事件
                              ▼
                    App/src-tauri/src/commands.rs    ← Tauri IPC
                              │
                              ▼
                    App/webui/                        ← 前端展示
```

## 快速开始

```bash
# 1) 应用（前端 + 桌面）
cd App
npm install
npm run dev                # 浏览器预览
npm run tauri:dev          # Tauri 桌面（需要 Rust）

# 2) 推流模拟器
cd Tools
pip install -r requirements.txt
python -m push_streamer.cli --config config.example.yaml

# 3) 在 App「视频源管理」里新增
#    http://127.0.0.1:8080/cam1/index.m3u8
```

详细文档看 [`Document/`](./Document/)。
