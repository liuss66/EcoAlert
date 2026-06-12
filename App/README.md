# App

产品代码目录：Tauri 2 桌面端 + Vite 前端 + Rust 后端。

## 子目录

| 目录 | 说明 |
| --- | --- |
| `webui/` | 前端页面和浏览器预览逻辑 |
| `src-tauri/` | Rust 后端、Tauri 配置、桌面运行时 |
| `src-tauri/src/pipeline/` | 算法流水线骨架，真实算法待接 |
| `src-tauri/src/stream/` | 视频流接入骨架，RTSP / 转码待接 |

测试目录不预建空壳；补单元测试或集成测试时再按实际框架创建。

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
- RTSP 当前仍是占位提示；后续通过后端转 HLS 或接入 ffmpeg 实现
