# EcoAlert 接口契约

> 版本：v1.3  
> 日期：2026-06-13  
> 范围：Tauri commands、Tauri events、通知 Payload

---

## 1. 通用约定

- 前端通过 `@tauri-apps/api/core.invoke` 调用 commands。
- 后端通过 Tauri event 推送实时状态。
- 已登录接口均需通过内存登录态校验。
- 时间戳统一使用 Unix milliseconds。
- Rust 内部字段使用 `snake_case`，前端建议统一转换为 `camelCase` 或通过 serde rename 固化契约。
- 错误返回使用 `Result<T, String>`，前端展示 `message`。

---

## 2. 已实现 Commands

### 2.1 鉴权

| Command | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `login` | `{ password }` | `{ ok, token }` | 登录成功后写入内存登录态 |
| `logout` | `{}` | `{ ok }` | 清除内存登录态 |
| `check_auth` | `{}` | `{ ok }` | 检查当前会话 |
| `change_password` | `{ oldPassword, newPassword }` | `{ ok }` | 修改管理员密码 |

### 2.2 视频源

| Command | 参数 | 返回 |
| --- | --- | --- |
| `list_sources` | `{}` | `VideoSource[]` |
| `create_source` | `{ payload }` | `VideoSource` |
| `update_source` | `{ id, payload }` | `VideoSource` |
| `delete_source` | `{ id }` | `{ ok }` |
| `import_test_sources_from_folder` | `{ folderPath }` | `{ sources, imported, skipped }` |

`SourcePayload`：

```json
{
  "name": "4·24 域控",
  "url": "G:\\project\\EcoAlert\\Video\\4·24域控.mp4",
  "type": "mp4",
  "location": "G:\\project\\EcoAlert\\Video",
  "enabled": true,
  "groupId": "grp-domain",
  "order": 0
}
```

注意：当前 Rust 结构体接收 `group_id`，前端 mock 使用 `groupId`。实现时需要统一字段转换。

`import_test_sources_from_folder` 会递归扫描所选文件夹中的 `mp4 / m4v / mov / mkv / avi / webm` 文件，跳过已存在的相同路径，导入为启用状态的 `mp4` 视频源，并归入「测试视频」分组。导入时会移除旧版固定 HLS 测试源，避免本地测试依赖推流器。

### 2.3 分组

| Command | 参数 | 返回 |
| --- | --- | --- |
| `list_groups` | `{}` | `SourceGroup[]` |
| `create_group` | `{ payload }` | `SourceGroup` |
| `update_group` | `{ id, payload }` | `SourceGroup` |
| `delete_group` | `{ id }` | `{ ok }` |
| `reorder` | `{ items }` | `{ ok }` |

`GroupPayload`：

```json
{
  "name": "域控测试视频",
  "order": 1,
  "collapsed": false
}
```

`OrderItem`：

```json
{
  "id": "src-xxx",
  "order": 2,
  "groupId": "grp-a"
}
```

### 2.4 状态历史

| Command | 参数 | 返回 |
| --- | --- | --- |
| `report_scene_state` | `{ sourceId, person, light }` | `{ ok }` |
| `get_state_history` | `{ sourceId?, limit? }` | `{ ok, records, bySource }` |

### 2.5 系统

| Command | 参数 | 返回 |
| --- | --- | --- |
| `get_data_dir` | `{}` | `String` |
| `reset_all_app_data` | `{}` | `{ ok, sources }` |

`reset_all_app_data` 仅用于调试初始化，会清空视频源、分组、算法配置、ROI、通知配置、报警记录、通知历史、检测历史、状态历史和运行状态，并重建默认分组；登录密码保留。

---

## 3. 配置 / 运行 Commands

### 3.1 算法配置（后端已接入全局 / 通道级）

| Command | 参数 | 返回 |
| --- | --- | --- |
| `get_algorithm_config` | `{ sourceId? }` | `AlgorithmConfig` |
| `update_algorithm_config` | `{ sourceId?, payload }` | `AlgorithmConfig` |
| `delete_algorithm_config` | `{ sourceId }` | `{ ok }` |
| `get_effective_algorithm_config` | `{ sourceId }` | `{ config, scope }` |

### 3.2 ROI（配置读写与真实帧测试已实现）

| Command | 参数 | 返回 |
| --- | --- | --- |
| `get_roi_config` | `{ sourceId }` | `RoiConfig` |
| `update_roi_config` | `{ sourceId, payload }` | `RoiConfig` |
| `test_roi_config` | `{ sourceId, payload? }` | `{ ok, light, lightState, person, brightness, colorScore, motionScore, confidence, processMs, version }` |

`test_roi_config` 会对当前视频源 URL 调用 ffmpeg 抽取一帧 `160x90` RGB 图，同时生成灰度图，再用传入或已保存的 ROI 配置运行 `Detector::analyze_scene()`。灯光判断优先使用 ROI 内 `colorScore`：该值是亮度加权色度分数，暗像素会降权以抑制红外近黑区域和压缩噪声；彩色画面高于色彩阈值输出 `light=true / lightState=on`，低于色彩阈值输出 `light=false / lightState=off`；无 RGB 数据时才回退亮度阈值。调用环境需要可执行的 `ffmpeg`。

### 3.3 报警（生命周期骨架已实现，静默待实现）

| Command | 参数 | 返回 |
| --- | --- | --- |
| `list_alarms` | `{ status?, sourceId?, limit? }` | `AlarmRecord[]` |
| `get_channel_runtime_status` | `{ sourceId? }` | `ChannelRuntimeStatus[]` |
| `ack_alarm` | `{ alarmId, note? }` | `AlarmRecord` |
| `resolve_alarm` | `{ alarmId, note? }` | `AlarmRecord` |

### 3.4 通知（发送 / 历史 / 渠道绑定已实现骨架）

| Command | 参数 | 返回 |
| --- | --- | --- |
| `list_notification_targets` | `{}` | `NotificationTarget[]` |
| `create_notification_target` | `{ payload }` | `NotificationTarget` |
| `update_notification_target` | `{ id, payload }` | `NotificationTarget` |
| `delete_notification_target` | `{ id }` | `{ ok }` |
| `test_notification_target` | `{ id?, payload? }` | `NotificationRecord` |
| `list_notification_history` | `{ sourceId?, event?, ok?, limit? }` | `NotificationRecord[]` |
| `resend_notification` | `{ recordId }` | `NotificationRecord` |
| `verify_channel_credentials` | `{ channelType, appId, appSecret }` | `{ ok, message }` |
| `start_oauth_binding` | `{ channelType, appId, appSecret }` | `{ sessionId, port, authUrl, qrData }` |
| `check_oauth_status` | `{ sessionId, appId, appSecret }` | `{ status }` 或 `{ status, accessToken, chats }` |

说明：

- `channelType = webhook`：普通 Webhook / HTTP POST 模式，使用 `url`、`method`、`headers`、`bodyTemplate`。
- `channelType = feishu | wechat_work | qqbot`：平台 API 凭证模式，使用 `appId`、`appSecret` 等字段自动获取并缓存 access token。
- `start_oauth_binding` / `check_oauth_status` 当前仅支持飞书扫码授权；企业微信和 QQ 当前使用凭证验证。
- 平台 API 模式内置默认文本 payload；填写 `bodyTemplate` 时仍按模板变量替换。

### 3.5 安全

| Command | 参数 | 返回 |
| --- | --- | --- |
| `get_security_config` | `{}` | `SecurityConfig` |
| `update_security_config` | `{ payload }` | `SecurityConfig` |

### 3.6 待实现 Commands

以下接口仍是需求目标，当前未在 `tauri::generate_handler!` 注册。

| Command | 参数 | 返回 |
| --- | --- | --- |
| `mute_source` | `{ sourceId, untilTs, reason? }` | `{ ok }` |
| `export_config` | `{ includeSecrets }` | `String 或文件路径` |
| `import_config` | `{ payload, dryRun }` | `{ ok, diff, warnings }` |

---

## 4. Events

### 4.1 已实现

| Event | Payload | 频率 |
| --- | --- | --- |
| `ecoalert://event` | `{ level, text, ts }` | 不定 |
| `ecoalert://status` | `ChannelStatus[]` | 3s |
| `ecoalert://runtime_status` | `ChannelRuntimeStatus[]` | 3s 或状态变化 |
| `ecoalert://scene_state` | `{ source_id, person, light, light_state, alarm, alarm_status, light_confidence, color_score, motion_score, ts }` | 每次检测完成 |
| `ecoalert://alarm` | `{ alarm_id, source_id, status, event, ts }` | 报警状态变化 |
| `ecoalert://notification` | `{ record_id, target_id, event, ok, status, error, ts }` | 通知发送完成 |
| `ecoalert://algorithm_schedule` | `{ source_id, action, reason, latency_ms, ts }` | 算法调度；`action` 包含 `run_simple`、`skip`、`frame_error` |

### 4.2 待实现

| Event | Payload | 触发 |
| --- | --- | --- |
| `ecoalert://sources` | `VideoSource[]` | 视频源变更 |

---

## 5. 数据对象

### 5.1 VideoSource

```json
{
  "id": "src-xxx",
  "name": "4·24 域控",
  "url": "http://127.0.0.1:8080/cam-1/index.m3u8",
  "type": "hls",
  "location": "Video/4·24域控.mp4",
  "enabled": true,
  "groupId": "grp-domain",
  "order": 0,
  "createdAt": 1781270400000
}
```

### 5.2 SceneState

```json
{
  "sourceId": "src-xxx",
  "person": false,
  "light": true,
  "lightState": "on",
  "alarm": false,
  "alarmStatus": "suspected",
  "frameSeq": 128,
  "confidence": 0.92,
  "source": "fused",
  "personConfidence": 0.21,
  "lightConfidence": 0.96,
  "reason": "vlm_recheck",
  "modelLatencyMs": 180,
  "lightBrightness": 213.4,
  "colorScore": 0.087,
  "motionScore": 0.018,
  "processMs": 2.7
}
```

`SceneState` 本体只表达画面事实；事件 payload 额外携带 `alarm / alarmStatus` 供前端展示。通知仍只由 `AlarmRecord` 状态变化触发。

当前 `source = simple` 时，`light` 是最终开关灯布尔结果，`lightState` / `light_state` 是便于 UI 直读的 `on/off` 字符串。该结果优先来自 `colorScore`：开灯时摄像头输出彩色图，关灯红外模式输出黑白图；`colorScore` 使用亮度加权色度，暗部彩噪不会像正常彩色亮区一样贡献权重；RGB 不可用时才回退到亮度阈值。`lightConfidence` 是当前开关状态判断置信度，不是单纯“关灯概率”。`person` 仍是帧差运动代理结果，不等同于真实人形检测。前端实时卡片会展示开关状态、`colorScore / motionScore / processMs`，用于判断是算法阈值问题还是事件链路问题。

### 5.3 AlarmRecord

```json
{
  "id": "alm-xxx",
  "sourceId": "src-xxx",
  "status": "alarm_active",
  "firstSeenAt": 1781270400000,
  "triggeredAt": 1781270460000,
  "acknowledgedAt": null,
  "resolvedAt": null,
  "acknowledgedBy": null,
  "note": null,
  "lastStateId": "rec-xxx"
}
```

`AlarmRecord` 表达业务报警生命周期。通知只能由 `AlarmRecord` 状态变化触发。

### 5.4 NotificationTarget

```json
{
  "id": "ntf-xxx",
  "name": "企业微信机器人",
  "enabled": true,
  "channelType": "webhook",
  "url": "https://example.com/webhook",
  "method": "POST",
  "headers": [
    { "name": "Content-Type", "value": "application/json" }
  ],
  "bodyTemplate": "{\"text\":\"{{source_name}} 无人亮灯\"}",
  "timeoutSec": 10,
  "retryCount": 2,
  "eventTypes": ["alarm_triggered", "alarm_resolved"],
  "cooldownSec": 1800,
  "createdAt": 1781270400000,
  "appId": "",
  "appSecret": "",
  "agentId": "",
  "chatId": "",
  "accessToken": "",
  "tokenExpiresAt": 0
}
```

字段说明：

| 字段 | 说明 |
| --- | --- |
| `channelType` | `webhook`、`feishu`、`wechat_work`、`qqbot`；空值默认按 `webhook` 处理 |
| `appId` | 飞书 App ID、企业微信 CorpID、QQ AppID |
| `appSecret` | 飞书 App Secret、企业微信 Secret、QQ ClientSecret |
| `agentId` | 企业微信应用 AgentID，仅 `wechat_work` API 模式需要 |
| `chatId` | 飞书 `chat_id`、企业微信 `touser`、QQ `group_openid` |
| `accessToken` / `tokenExpiresAt` | 后端内部缓存字段，token 即将过期时自动刷新 |

API 凭证模式示例：

```json
{
  "name": "飞书群通知",
  "enabled": true,
  "channelType": "feishu",
  "url": "",
  "method": "POST",
  "headers": [],
  "bodyTemplate": "",
  "timeoutSec": 10,
  "retryCount": 2,
  "eventTypes": ["alarm_triggered", "alarm_resolved"],
  "cooldownSec": 1800,
  "appId": "cli_xxx",
  "appSecret": "secret_xxx",
  "chatId": "oc_xxx"
}
```

### 5.5 AlgorithmConfig

```json
{
  "enabled": true,
  "developerMode": false,
  "scope": "source",
  "scopeId": "src-xxx",
  "activeWindows": [
    {
      "weekdays": [1, 2, 3, 4, 5],
      "start": "18:30",
      "end": "08:30",
      "timezone": "Asia/Shanghai"
    }
  ],
  "simpleIntervalSec": 1,
  "vlmIntervalSec": 300,
  "vlmEnabled": true,
  "vlmSkipWhenPerson": true,
  "personThreshold": 0.65,
  "lightThreshold": 0.7,
  "alarmHoldSec": 300,
  "alarmRecoverSec": 60,
  "recoverPolicy": "either",
  "vlmHourlyLimit": 12,
  "roiVersion": "roi-v1"
}
```

配置继承优先级：

```text
system < global < group < source
```

`get_effective_algorithm_config` 返回合并后的配置，并在 `sources` 中标明每个字段来自哪个层级。

`developerMode = true` 时，后端调度忽略 `activeWindows / exceptionWindows`，前端实时视频卡片显示 `scene-readout` 检测读数；关闭时隐藏检测读数。

### 5.6 ChannelRuntimeStatus

```json
{
  "sourceId": "src-xxx",
  "onlineStatus": "online",
  "algorithmStatus": "idle",
  "alarmStatus": "normal",
  "lastFrameAt": 1781270400000,
  "lastAlgorithmAt": 1781270410000,
  "lastError": null,
  "effectiveAlgorithmConfigScope": "source",
  "ts": 1781270420000
}
```

---

## 6. 通知 Payload

报警事件默认结构化 payload：

```json
{
  "event": "alarm_triggered",
  "source_id": "src-xxx",
  "source_name": "4·24 域控",
  "location": "Video/4·24域控.mp4",
  "person": false,
  "light": true,
  "alarm": true,
  "confidence": 0.92,
  "state_source": "fused",
  "alarm_id": "alm-xxx",
  "ts": 1781270400000
}
```

模板变量：

| 变量 | 说明 |
| --- | --- |
| `event` | 事件类型 |
| `source_id` | 视频源 ID |
| `source_name` | 视频源名称 |
| `location` | 位置 |
| `person` | 是否有人 |
| `light` | 是否亮灯 |
| `alarm` | 是否报警 |
| `confidence` | 总置信度 |
| `state_source` | `simple / vlm / fused` |
| `alarm_id` | 报警记录 ID |
| `ts` | 事件时间 |

模板安全规则：

- 模板只支持变量替换，不执行脚本。
- 未知变量渲染为空字符串。
- Header 中的敏感值不写入日志。
- 测试发送可展示最终 payload，但敏感字段必须脱敏。
- Webhook 模式默认按渠道生成文本消息：飞书 `msg_type=text`、企业微信 `msgtype=text`、QQ `msg_type=0`；未识别渠道发送结构化 JSON。
- API 凭证模式默认调用平台官方发送接口，并在通知目标中缓存 access token。

---

## 7. 配置导入冲突策略

| 场景 | 默认策略 |
| --- | --- |
| 同 ID 配置 | 覆盖，用户可选择生成新 ID |
| 视频源 URL 相同但 ID 不同 | 标记潜在重复，要求确认 |
| 通知目标缺失密钥 | 导入但默认禁用 |
| 历史记录导入 | 默认不导入，需单独选择 |
| dry-run | 只返回差异和警告，不修改当前配置 |

---

## 8. 兼容性要求

- 新增字段必须提供默认值，旧配置可正常加载。
- 删除字段前必须保留至少一个版本的兼容读取。
- 前端 mock 数据结构要和 Tauri 返回结构保持一致。
- 新增 command 需要同步更新 `App/webui/src/api.js`。
