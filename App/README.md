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

浏览器预览只验证 UI、视频播放和 localStorage 模拟数据，不再伪造 person / light 检测事件。真实算法事件需要运行 Tauri 桌面端，并确保本机可执行 `ffmpeg`。

## 本地 HLS 联调

默认视频源指向 `http://127.0.0.1:8080/cam-N/index.m3u8`，与 `Tools/push_streamer` 的 `config.example.yaml` 保持一致。release 包播放本地 HLS 时，`tauri.conf.json` 需要允许 `connect-src` / `media-src` 访问本机地址，并保留针对 WebView2 私网请求限制的 `additionalBrowserArgs` feature 配置。

实时监控页的“诊断”按钮调用 Rust 后端访问 `cam-1`，用于确认推流器和本机网络可达；它不等同于 WebView/HLS.js 播放成功。

## 当前检测状态

- 灯光检测：已使用真实 RGB 抽帧，优先按“开灯彩色、关灯红外黑白”的色彩分数做 EMA + 磁滞阈值判断，实时卡片会显示色彩分数和置信度；亮度规则保留为兜底。
- 人员检测：当前不是人形模型，只是低分辨率帧差运动代理；视频中有人静止时可能判为无人，画面整体变化时也可能误判为有人。
- 开发者模式：开启后忽略算法生效时段，并显示视频卡片检测读数；关闭时隐藏检测读数。
- ROI 默认：灯光 ROI 默认全屏，可按通道缩小到指定区域。
- 输出链路：Tauri 后端每次检测完成都会推送 `ecoalert://scene_state`；默认全天运行。若开发者模式下显示“检测：抽帧失败”或“检测：未运行”，优先看 tooltip、ffmpeg 是否在 PATH、HLS URL 是否可达和算法启用时段配置。

## 数据存储

- Windows: `%APPDATA%\com.ecoalert.monitor\`
- macOS: `~/Library/Application Support/com.ecoalert.monitor/`
- Linux: `~/.local/share/com.ecoalert.monitor/`

## 视频源类型

- HLS / MP4 / 摄像头 / RTSP
- RTSP 当前仍是占位提示；后续通过后端转 HLS 或接入 ffmpeg 实现
