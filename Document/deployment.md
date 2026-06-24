# EcoAlert 部署与升级

> 版本：v1.3
> 日期：2026-06-24

---

## 目录

- [1. 环境要求](#1-环境要求)
- [2. 开发运行](#2-开发运行)
- [3. 打包](#3-打包)
- [4. 数据目录](#4-数据目录)
- [5. 升级策略](#5-升级策略)
- [6. 通知与外部服务配置](#6-通知与外部服务配置)
- [7. VLM 与隐私配置](#7-vlm-与隐私配置)
- [8. YOLO 服务部署](#8-yolo-服务部署)
- [9. 常见问题](#9-常见问题)
- [10. 发布检查清单](#10-发布检查清单)

---

## 1. 环境要求

| 组件 | 版本 / 要求 |
| --- | --- |
| Node.js | 18+ |
| npm | 随 Node.js 安装 |
| Rust | 1.77+ |
| Tauri | 2.x |
| WebView2 | Windows 10+ / 11 通常自带 |
| ffmpeg | 后端抽帧、RTSP / HLS 转码依赖 |
| ffprobe | 获取本地视频时长，支持循环播放时间点映射 |
| Python 3.10+ | 仅在启用独立 YOLO WebSocket 服务时需要 |

---

## 2. 开发运行

### 2.1 前端浏览器预览

```bash
cd App
npm install
npm run dev
```

默认地址：

```text
http://localhost:1420
```

浏览器预览模式使用 mock 数据，不依赖 Rust 后端；它只验证 UI、视频播放和 localStorage 持久化，不再伪造 `person / light` 检测事件。

### 2.2 Tauri 桌面开发

```bash
cd App
npm install
npm run tauri:dev
```

Tauri 模式会启动 Rust 后端，数据写入本机应用数据目录。真实检测、报警状态机和通知发送都只在 Tauri 模式运行。

### 2.3 本地测试视频

Tauri 桌面端可直接导入本地视频文件，不再要求先推流：

1. 登录 EcoAlert。
2. 进入「基础设置 -> 调试」。
3. 打开「测试视频源」开关，并在弹出的窗口中选择文件夹；也可点击「选择测试视频文件夹」重新导入。
4. 选择包含 `mp4 / mov / mkv / avi / webm` 的文件夹。
5. 系统会递归扫描并导入为「测试视频」分组下的 MP4 视频源，实时监控页循环播放。

新增视频源弹窗也可点击「选择文件」单独添加本地视频文件。后端检测链路保存原始文件路径，使用 ffmpeg 直接抽帧。

运行时前端会把 MP4 播放器的当前时间、播放 / 暂停状态同步给后端。后端在 15 秒内收到过有效进度时，优先按该时间点抽帧；否则回退到按检测次数估算时间点。播放器跳转、暂停和继续播放都会触发立即同步。

### 2.4 可选 HLS 推流工具

`push_streamer` 已支持 ffmpeg 循环推流和 HLS 分片，仅在需要验证 HLS 链路时使用：

```bash
cd Tools
python -m push_streamer.cli --config config.example.yaml
```

然后在 App 中使用 HLS 源，例如：

```text
http://127.0.0.1:8080/cam-1/index.m3u8
```

### 2.5 本地视频 / HLS 与 release 包

release 包播放本地视频文件时需要 CSP 允许 `asset:` 媒体源；当前 `tauri.conf.json` 的 `media-src` 已包含 `asset:`。HLS 链路仍需满足：

- `Tools/push_streamer` 正在运行，并且对应 `cam-N/index.m3u8` 返回 `200`。
- `App/src-tauri/tauri.conf.json` 的 CSP 允许 `connect-src` 和 `media-src` 访问 `http://127.0.0.1:*`、`http://localhost:*`。
- Windows WebView2 / Chromium 对安全上下文访问本机私网资源有额外限制，当前仅通过 `additionalBrowserArgs` 关闭 `BlockInsecurePrivateNetworkRequests` 和 `InsecurePrivateNetworkSubresourceRequests` 两个本地推流相关 feature；不要在 release 包使用 `--disable-web-security`。
- 后端诊断命令 `probe_url` 使用 Rust `reqwest` 访问 URL，只能证明本机网络和推流器可达，不能替代 WebView/HLS.js 播放验证。

---

## 3. 打包

```bash
cd App
npm install
npm run tauri:build
```

产物位置由 Tauri 默认配置决定，通常位于：

```text
App/src-tauri/target/release/bundle/
```

Windows 常见产物：

- `.msi`
- `.exe`

---

## 4. 数据目录

| OS | 路径 |
| --- | --- |
| Windows | `%APPDATA%\com.ecoalert.monitor\` |
| macOS | `~/Library/Application Support/com.ecoalert.monitor/` |
| Linux | `~/.local/share/com.ecoalert.monitor/` |

文件清单：

| 文件 | 说明 |
| --- | --- |
| `sources.json` | 视频源和分组 |
| `auth.json` | 管理员密码哈希 |
| `state_history.json` | 状态历史 |
| `algorithm_config.json` | 算法配置 |
| `roi_config.json` | ROI 和标定 |
| `alarm_records.json` | 报警记录 |
| `notification_config.json` | 通知配置 |
| `notification_history.json` | 通知历史 |
| `security_config.json` | 隐私安全配置 |

---

## 5. 升级策略

### 5.1 配置版本

所有新增配置文件应包含：

```json
{
  "schema_version": 1
}
```

应用启动时按以下流程加载：

1. 检查文件是否存在，不存在则创建默认配置。
2. 检查 `schema_version`。
3. 如版本低于当前版本，执行迁移。
4. 迁移前创建备份文件。
5. 迁移失败时保留原文件并记录错误日志。

### 5.2 备份命名

```text
<file>.bak.<yyyyMMddHHmmss>
```

示例：

```text
sources.json.bak.20260613023000
```

### 5.3 回滚

第一版不做自动回滚。手动回滚步骤：

1. 关闭 EcoAlert。
2. 找到数据目录。
3. 将当前配置文件改名保留。
4. 将 `.bak` 文件恢复为原文件名。
5. 重新启动 EcoAlert。

---

## 6. 通知与外部服务配置

通知只要求 HTTP 可达，不绑定具体平台。

部署前需要确认：

- 通知目标 URL 可从本机访问。
- 代理、防火墙、证书策略允许请求。
- Webhook token 等密钥已配置。
- 测试发送返回成功。

通知失败不应影响本地报警。

---

## 7. VLM 与隐私配置

外部 VLM 默认关闭。当前已支持 OpenAI Chat Completions 兼容视觉接口、连接测试、视频抽帧视觉测试、用量读取和可选费用估算。启用前需要确认：

- 是否允许截图发送到第三方 API。
- 是否需要对人员区域打码。
- 截图是否落盘。
- 截图保留天数。
- API Key 是否安全存储。

推荐生产默认：

| 配置 | 值 |
| --- | --- |
| 外部 VLM | 关闭 |
| VLM 截图落盘 | 关闭 |
| 通知包含图片 | 关闭 |
| 人员区域打码 | 开启 |

当前代码尚未实现发送前打码，因此生产接入时不能仅依赖上表建议，必须确认所连接服务允许接收原始截图。API Key 保存在本机 `algorithm_config.json`，部署机器的数据目录权限应限制为当前运行用户。

---

## 8. YOLO 服务部署

YOLO 是可选的独立 Python WebSocket 服务，启用后替代运动代理作为人员检测主结果。

```powershell
pip install -r Algo/YOLO/requirements.txt
./scripts/start_yolo.ps1
Invoke-RestMethod http://localhost:8090/health
```

部署要求：

- 模型权重单独放入 `Algo/YOLO/model/`，权重不提交到 Git。
- 默认只监听 `127.0.0.1`；开放到局域网时必须配置 `ECOALERT_YOLO_TOKEN`。
- App 默认连接 `ws://localhost:8090`，协议详见 `Algo/YOLO/README.md`。
- YOLO 服务故障会记录 `yolo_error` 通知事件，并抑制对应通道的新报警。
- 当前客户端每次检测完成后主动断开 WebSocket，避免服务端空闲 keepalive 超时；服务端仍需支持标准 `/ws` 接口。

---

## 9. 常见问题

### 9.1 HLS 视频无法播放

检查：

- HLS URL 是否能访问，例如 `http://127.0.0.1:8080/cam-1/index.m3u8`。
- 推流工具是否运行。
- 防火墙是否阻止本机端口。
- 浏览器是否支持 HLS；Chrome 依赖 `hls.js`。
- release 包是否保留了本地 HLS CSP 白名单和私网请求 feature 配置。
- 点击实时监控页“诊断”只能确认后端可达；若诊断成功但视频仍失败，应继续检查 WebView 的 HLS 请求和 `.ts` 分片。

### 9.1.1 本地视频文件无法播放

检查：

- 文件路径是否仍然存在，移动或删除文件后需要重新选择。
- 文件扩展名是否为 `mp4 / mov / mkv / avi / webm`。
- release 包是否保留 `asset:` 的 `media-src` CSP 白名单。
- 播放正常但检测失败时，确认 `ffmpeg -version` 可执行。

### 9.2 Tauri 启动失败

检查：

- Rust 工具链是否安装。
- `npm install` 是否完成。
- WebView2 是否可用。
- `App/src-tauri/tauri.conf.json` 是否有效。

### 9.3 ffmpeg 不可用

检查：

```bash
ffmpeg -version
```

若命令不可用，将 ffmpeg 加入系统 `PATH`。

### 9.4 数据异常

处理顺序：

1. 在「基础设置」中查看数据目录。
2. 备份当前数据目录。
3. 检查 JSON 文件格式。
4. 恢复最近 `.bak` 文件。
5. 如果仍失败，删除对应配置文件让应用重建默认值。

---

## 10. 发布检查清单

- `npm run dev` 可启动。
- `npm run tauri:dev` 可启动。
- `npm run tauri:build` 可完成。
- 登录、视频源 CRUD、分组 CRUD 可用。
- 本地视频文件夹导入后，dev 和 release 包均能循环播放测试视频。
- 如验证 HLS 链路，本地推流器启动后 dev 和 release 包均能播放 `cam-1` HLS。
- 状态历史能正常写入。
- 打包命令成功。
- 安装包在目标系统能启动。
- 数据目录能创建配置文件。
- 通知测试发送成功。
- 外部 VLM 默认关闭。
- 启用 YOLO 时，健康检查、App 内连接测试、检测框显示和服务故障抑制均已验证。
- 启用 VLM 时，连接测试、视觉测试、JSON 返回格式、边界框显示和小时上限均已验证。
- 本地 MP4 播放、暂停、跳转和循环后，检测画面仍与播放器时间点一致。
- 视频容器全屏后仍保留 YOLO / VLM 检测框。
- 文档版本和变更日志已更新。
