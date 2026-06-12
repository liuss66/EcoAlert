# App

Tauri 桌面应用（Tauri 2 + Vite + 原生 JS + Rust 后端）。

## 子目录

| 目录 | 说明 |
| --- | --- |
| `webui/` | 前端代码（Vite 编译） |
| `src-tauri/` | Rust 后端 + 桌面运行时 |
| `src-tauri/src/pipeline/` | 视频处理流水线（解码 → 检测 → 分析 → 告警） |
| `src-tauri/src/stream/` | 推流承接（HLS / RTMP，Tools 推来的实时流在这里入站） |
| `tests/` | App 端集成测试与测试数据 |

## 启动

```bash
# 一次性
npm install

# 浏览器预览（不装 Rust 工具链）
npm run dev
# → http://localhost:1420

# Tauri 桌面 app（需要 Rust 1.77+）
npm run tauri:dev      # 开发
npm run tauri:build    # 打包 .msi / .exe
```

## 数据存储

- Windows: `%APPDATA%\com.ecoalert.monitor\`
- macOS: `~/Library/Application Support/com.ecoalert.monitor/`
- Linux: `~/.local/share/com.ecoalert.monitor/`

## 视频源类型

- HLS / MP4 / 摄像头 / RTSP
- RTSP 需 `src-tauri/src/stream/rtmp.rs`（实际是收 RTMP，Tools 用 ffmpeg 转推）
