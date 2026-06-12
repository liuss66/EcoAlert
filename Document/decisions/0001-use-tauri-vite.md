# ADR-0001 使用 Tauri 2 + Vite 构建桌面应用

## 状态

已接受

## 背景

EcoAlert 需要本地桌面形态，具备文件读写、后台任务、视频状态推送和本地配置能力。同时前端需要快速迭代和较好的 UI 开发体验。

## 决策

使用 Tauri 2 作为桌面运行时，使用 Vite + 原生 JavaScript 构建前端。

## 理由

- Tauri 包体较小，适合本地工具类应用。
- Rust 后端便于处理本地文件、后台任务和后续视频 / 算法流水线。
- Vite 启动快，浏览器预览模式可以在不安装 Rust 的情况下开发 UI。
- Tauri event / command 足够覆盖当前 IPC 需求。

## 后果

- 需要维护 Rust 与前端之间的数据契约。
- Windows 依赖 WebView2。
- 视频和算法相关能力需要在 Rust 后端逐步补齐。

