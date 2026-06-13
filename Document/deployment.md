# EcoAlert 部署与升级

> 版本：v1.0  
> 日期：2026-06-13

---

## 1. 环境要求

| 组件 | 版本 / 要求 |
| --- | --- |
| Node.js | 18+ |
| npm | 随 Node.js 安装 |
| Rust | 1.77+ |
| Tauri | 2.x |
| WebView2 | Windows 10+ / 11 通常自带 |
| ffmpeg | 推流工具、RTSP / HLS 转码依赖 |

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

浏览器预览模式使用 mock 数据，不依赖 Rust 后端。

### 2.2 Tauri 桌面开发

```bash
cd App
npm install
npm run tauri:dev
```

Tauri 模式会启动 Rust 后端，数据写入本机应用数据目录。

### 2.3 推流工具

```bash
cd Tools
pip install -r requirements.txt
python -m push_streamer.cli --video ../Video/sample_01_meeting.mp4 --loop
```

`push_streamer` 已支持 ffmpeg 循环推流和 HLS 分片。App 联调优先使用固定映射配置启动：

```bash
cd Tools
python -m push_streamer.cli --config config.example.yaml
```

然后在 App 中使用 HLS 源，例如：

```text
http://127.0.0.1:8080/cam-1/index.m3u8
```

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

外部 VLM 默认关闭。启用前需要确认：

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

---

## 8. 常见问题

### 8.1 HLS 视频无法播放

检查：

- HLS URL 是否能在浏览器访问。
- 推流工具是否运行。
- 防火墙是否阻止本机端口。
- 浏览器是否支持 HLS；Chrome 依赖 `hls.js`。

### 8.2 Tauri 启动失败

检查：

- Rust 工具链是否安装。
- `npm install` 是否完成。
- WebView2 是否可用。
- `App/src-tauri/tauri.conf.json` 是否有效。

### 8.3 ffmpeg 不可用

检查：

```bash
ffmpeg -version
```

若命令不可用，将 ffmpeg 加入系统 `PATH`。

### 8.4 数据异常

处理顺序：

1. 在系统设置中查看数据目录。
2. 备份当前数据目录。
3. 检查 JSON 文件格式。
4. 恢复最近 `.bak` 文件。
5. 如果仍失败，删除对应配置文件让应用重建默认值。

---

## 9. 发布检查清单

- `npm run dev` 可启动。
- `npm run tauri:dev` 可启动。
- 登录、视频源 CRUD、分组 CRUD 可用。
- 状态历史能正常写入。
- 打包命令成功。
- 安装包在目标系统能启动。
- 数据目录能创建配置文件。
- 通知测试发送成功。
- 外部 VLM 默认关闭。
- 文档版本和变更日志已更新。
