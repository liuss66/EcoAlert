# EcoAlert 接口契约

> 版本：v1.0  
> 日期：2026-06-13  
> 范围：Tauri commands、Tauri events、通知 Webhook payload

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

`SourcePayload`：

```json
{
  "name": "A栋办公室",
  "url": "http://127.0.0.1:8080/cam1/index.m3u8",
  "type": "hls",
  "location": "A栋 2F",
  "enabled": true,
  "groupId": "grp-default",
  "order": 0
}
```

注意：当前 Rust 结构体接收 `group_id`，前端 mock 使用 `groupId`。实现时需要统一字段转换。

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
  "name": "A栋办公",
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

---

## 3. 待实现 Commands

### 3.1 算法配置

| Command | 参数 | 返回 |
| --- | --- | --- |
| `get_algorithm_config` | `{ sourceId? }` | `AlgorithmConfig` |
| `update_algorithm_config` | `{ sourceId?, payload }` | `AlgorithmConfig` |
| `get_effective_algorithm_config` | `{ sourceId }` | `{ config, sources }` |

### 3.2 ROI

| Command | 参数 | 返回 |
| --- | --- | --- |
| `get_roi_config` | `{ sourceId }` | `RoiConfig` |
| `update_roi_config` | `{ sourceId, payload }` | `RoiConfig` |
| `test_roi_config` | `{ sourceId, payload? }` | `{ ok, light, brightness, confidence }` |

### 3.3 报警

| Command | 参数 | 返回 |
| --- | --- | --- |
| `list_alarms` | `{ status?, sourceId?, limit? }` | `AlarmRecord[]` |
| `get_channel_runtime_status` | `{ sourceId? }` | `ChannelRuntimeStatus[]` |
| `ack_alarm` | `{ alarmId, note? }` | `AlarmRecord` |
| `resolve_alarm` | `{ alarmId, note? }` | `AlarmRecord` |
| `mute_source` | `{ sourceId, untilTs, reason? }` | `{ ok }` |

### 3.4 通知

| Command | 参数 | 返回 |
| --- | --- | --- |
| `list_notification_targets` | `{}` | `NotificationTarget[]` |
| `create_notification_target` | `{ payload }` | `NotificationTarget` |
| `update_notification_target` | `{ id, payload }` | `NotificationTarget` |
| `delete_notification_target` | `{ id }` | `{ ok }` |
| `test_notification_target` | `{ id? 或 payload }` | `{ ok, status?, error? }` |
| `list_notification_history` | `{ filter }` | `NotificationRecord[]` |
| `resend_notification` | `{ recordId }` | `NotificationRecord` |

### 3.5 安全与配置

| Command | 参数 | 返回 |
| --- | --- | --- |
| `get_security_config` | `{}` | `SecurityConfig` |
| `update_security_config` | `{ payload }` | `SecurityConfig` |
| `export_config` | `{ includeSecrets }` | `String 或文件路径` |
| `import_config` | `{ payload, dryRun }` | `{ ok, diff, warnings }` |

---

## 4. Events

### 4.1 已实现

| Event | Payload | 频率 |
| --- | --- | --- |
| `ecoalert://event` | `{ level, text, ts }` | 不定 |
| `ecoalert://status` | `ChannelStatus[]` | 3s |
| `ecoalert://scene_state` | `{ source_id, person, light, ts }` | 变化时或心跳 |

### 4.2 待实现

| Event | Payload | 触发 |
| --- | --- | --- |
| `ecoalert://sources` | `VideoSource[]` | 视频源变更 |
| `ecoalert://runtime_status` | `ChannelRuntimeStatus[]` | 运行状态变化或周期推送 |
| `ecoalert://alarm` | `{ alarm_id, source_id, status, event, ts }` | 报警状态变化 |
| `ecoalert://notification` | `{ target_id, event, ok, status, error, ts }` | 通知发送完成 |
| `ecoalert://algorithm_schedule` | `{ source_id, action, reason, latency_ms, ts }` | 算法调度 |

---

## 5. 数据对象

### 5.1 VideoSource

```json
{
  "id": "src-xxx",
  "name": "A栋办公室",
  "url": "http://127.0.0.1:8080/cam1/index.m3u8",
  "type": "hls",
  "location": "A栋 2F",
  "enabled": true,
  "groupId": "grp-default",
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
  "frameSeq": 128,
  "confidence": 0.92,
  "source": "fused",
  "personConfidence": 0.21,
  "lightConfidence": 0.96,
  "reason": "vlm_recheck",
  "modelLatencyMs": 180
}
```

`SceneState` 只表达画面事实，不直接触发通知。

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
  "createdAt": 1781270400000
}
```

### 5.5 AlgorithmConfig

```json
{
  "enabled": true,
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
  "simpleIntervalSec": 10,
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

## 6. 通知 Webhook Payload

默认 payload：

```json
{
  "event": "alarm_triggered",
  "source_id": "src-xxx",
  "source_name": "A栋办公室",
  "location": "A栋 2F",
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
