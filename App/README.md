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

## 本地视频联调

Tauri 桌面端支持直接选择本地视频文件作为 `mp4` 视频源。新增视频源弹窗可点击“选择文件”；开发者模式的「调试」页保留“测试视频源”开关，开启时选择视频文件夹并递归扫描 `mp4 / mov / mkv / avi / webm` 文件，自动导入为循环播放的测试视频源。关闭开关会移除已导入的测试视频源，旁边的“选择测试视频文件夹”按钮可用于重新导入。

`Tools/push_streamer` 仍可作为可选 HLS 链路联调工具。实时监控页的“诊断”按钮只用于检查 `cam-1` 这类本地 HLS URL 的后端可达性，不影响本地视频文件导入流程。

## 当前检测状态

- 灯光检测：已使用真实 RGB 抽帧，优先按”开灯彩色、关灯红外黑白”的亮度加权色度分数做 EMA + 单阈值判断，暗部彩噪会被降权；实时卡片会显示”灯光：开灯 / 关灯”、色彩分数和置信度；亮度规则仅在无 RGB 数据时兜底。
- 人员检测：当前不是人形模型，只是低分辨率帧差运动代理；视频中有人静止时可能判为无人，画面整体变化时也可能误判为有人。
- 开发者模式：开启后忽略算法生效时段，并显示视频卡片检测读数；关闭时隐藏检测读数。
- ROI 默认：灯光 ROI 默认全屏，可按通道缩小到指定区域；ROI 色彩阈值默认 `0.015`。
- 输出链路：Tauri 后端每次检测完成都会推送 `ecoalert://scene_state`；默认全天运行。若开发者模式下显示“检测：抽帧失败”或“检测：未运行”，优先看 tooltip、ffmpeg 是否在 PATH、视频文件路径 / HLS URL 是否可达和算法启用时段配置。

## 数据存储

- Windows: `%APPDATA%\com.ecoalert.monitor\`
- macOS: `~/Library/Application Support/com.ecoalert.monitor/`
- Linux: `~/.local/share/com.ecoalert.monitor/`

## 视频源类型

- HLS / MP4 / 摄像头 / RTSP
- RTSP 当前仍是占位提示；后续通过后端转 HLS 或接入 ffmpeg 实现
