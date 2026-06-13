# EcoAlert 架构设计

> 版本：v1.0  
> 日期：2026-06-13  
> 对应需求：[`requirements.md`](./requirements.md)

---

## 1. 架构目标

EcoAlert 是本地桌面视频监控应用，核心目标是：

- 本地运行，不依赖云端服务。
- 支持多路视频源展示、分组、状态历史和报警。
- 算法可插拔，前端不绑定具体模型。
- 通过简单模型高频巡检、VLM 低频补漏控制成本。
- 支持通知、报警生命周期、ROI 标定和配置迁移。

---

## 2. 总体架构

```mermaid
flowchart LR
    Video["Video/*.mp4 测试视频"] --> WebUI["webui/src/main.js"]
    Video --> Tools["Tools/push_streamer"]
    Tools -- HLS --> WebUI
    Tools -.后续接入.-> Stream["App/src-tauri/src/stream"]
    Camera["摄像头 / RTSP / HLS / MP4"] --> Stream
    Stream --> Scheduler["pipeline/scheduler"]
    Scheduler --> Decoder["pipeline/decoder"]
    Decoder --> Detector["pipeline/detector"]
    Detector --> Analyzer["pipeline/analyzer"]
    Analyzer --> Alerts["pipeline/alerts"]
    Alerts --> State["state.rs"]
    State --> Store["store.rs + JSON files"]
    State --> Events["Tauri events"]
    Commands["commands.rs"] <--> Store
    WebUI <--> Commands
    Events --> WebUI
    WebUI --> UI["实时监控 / 总览 / 设置 / 日志"]
    Alerts --> Notify["notification service"]
    Notify --> Webhook["Webhook / HTTP POST"]
```

---

## 3. 模块划分

| 模块 | 路径 | 职责 | 当前状态 |
| --- | --- | --- | --- |
| 前端 UI | `App/webui/` | 登录、实时监控、总览、视频管理、设置、日志 | 已实现基础功能 |
| Tauri Commands | `App/src-tauri/src/commands.rs` | 前后端 IPC、鉴权、CRUD、状态查询 | 已实现基础功能 |
| 应用状态 | `App/src-tauri/src/state.rs` | 全局状态、后台 ticker、事件推送、历史落库 | 已实现 mock 状态 |
| 数据模型 | `App/src-tauri/src/store.rs` | 视频源、分组、状态历史、JSON 持久化 | 已实现基础模型 |
| 视频输入 | `App/src-tauri/src/stream/` | HLS / RTMP 接入骨架 | 部分占位 |
| 算法调度 | `App/src-tauri/src/pipeline/scheduler.rs` | 启用时段、周期、VLM 队列、并发限制、跳过原因 | 待实现 |
| 处理流水线 | `App/src-tauri/src/pipeline/` | 解码、检测、分析、告警 | 骨架完成，真实算法待接 |
| 推流工具 | `Tools/push_streamer/` | 用本地视频模拟实时 HLS 源，供 App 页面联调 | 已实现 ffmpeg + HLS |
| 文档 | `Document/` | 需求、架构、接口、部署、ADR、变更日志 | 本文档集 |

### 3.1 目录设计原则

项目目录按“职责真实存在”组织，不为未来想象中的模块预建空目录。

| 顶层目录 | 保留原因 |
| --- | --- |
| `App/` | 唯一产品代码目录，包含前端、Tauri 后端和桌面配置 |
| `Document/` | 集中放产品和工程设计文档，避免说明散落到代码目录 |
| `Video/` | 测试视频平铺存放，方便开发时复制路径 |
| `Tools/` | 放开发辅助工具；当前包含本地 HLS 推流器和联调配置 |

不再维护 `Video/samples/long/benchmark` 这类多级目录；测试视频数量不大时，根目录平铺比分类目录更容易使用。测试目录、图片目录、工具子模块等只有在真实文件出现时再创建。

---

## 4. 运行时数据流

### 4.1 当前可运行路径

```mermaid
sequenceDiagram
    participant UI as Web UI
    participant Cmd as Tauri commands
    participant State as AppState
    participant File as JSON files

    UI->>Cmd: login(password)
    Cmd->>State: verify auth
    State-->>Cmd: ok
    Cmd-->>UI: token

    UI->>Cmd: list_sources()
    Cmd->>State: read sources
    State->>File: sources.json
    Cmd-->>UI: VideoSource[]

    State-->>UI: ecoalert://status
    State-->>UI: ecoalert://scene_state
    State->>File: state_history.json
```

### 4.2 目标算法路径

```mermaid
sequenceDiagram
    participant Stream as stream
    participant Decoder as decoder
    participant Simple as simple model
    participant VLM as VLM
    participant Alarm as alarm engine
    participant Notify as notifier
    participant UI as Web UI

    Stream->>Decoder: FramePacket
    Decoder->>Simple: DecodedFrame + ROI
    Simple-->>Alarm: person/light/confidence
    alt simple model found person
        Alarm-->>UI: scene_state person=true
    else no person and light on
        Alarm->>VLM: optional recheck
        VLM-->>Alarm: corrected scene state
        Alarm-->>UI: alarm event
        Alarm->>Notify: alarm_triggered
    end
```

---

## 5. 核心设计

### 5.1 算法分层

| 层级 | 频率 | 作用 | 输出 |
| --- | --- | --- | --- |
| ROI 灯光规则 | 5-15 秒 | 判断灯亮 / 灯灭 | `light`, `light_confidence` |
| 轻量人形检测 | 5-15 秒 | 判断是否有人 | `person`, `person_confidence` |
| 动态目标识别 | 5-15 秒 | 变化触发器，不直接判断有人 | `motion`, `confidence` |
| VLM 复核 | 2-10 分钟或按条件触发 | 修补简单模型漏检 | `person`, `light`, `reason` |
| 融合 / 状态机 | 每次状态变化 | 报警生命周期、冷却、恢复 | `AlarmRecord` |

关键规则：

- 简单模型识别到有人时，不调用 VLM。
- VLM 只在无人 + 亮灯且需要复核时调用。
- 算法启用时段外不产生新报警。
- 视频离线不能被解释为无人 + 亮灯。

### 5.2 调度器职责

算法调度器是独立模块，不应把调度逻辑分散到 detector、analyzer 或 state 中。

| 职责 | 说明 |
| --- | --- |
| 时段判断 | 判断当前通道是否处于算法启用时段或例外时段 |
| 周期控制 | 分别控制简单模型周期和 VLM 周期 |
| 配置合并 | 计算系统默认、全局、分组、通道四层配置后的有效配置 |
| VLM 队列 | 控制 VLM 并发、冷却、每小时调用上限 |
| 跳过原因 | 记录 `schedule_disabled / source_offline / simple_hit_person / cooldown / concurrency_limit` |
| 运行状态 | 更新 `algorithm_status`、`last_algorithm_at`、`last_error` |

配置继承顺序：

```text
system default < global < group < source
```

后一级只覆盖显式设置的字段，未设置字段继承前一级。UI 需要展示每项配置的来源，并允许恢复为继承值。

### 5.3 状态边界

系统中有四类状态，不能混用：

| 状态 | 数据对象 | 含义 | 触发下游 |
| --- | --- | --- | --- |
| 画面事实 | `SceneState` | 算法看到的人、灯、置信度、来源 | 状态历史、报警判定 |
| 业务报警 | `AlarmRecord` | 是否构成疑似 / 正式 / 已确认 / 已恢复报警 | UI 报警、通知 |
| 视频在线 | `ChannelRuntimeStatus.online_status` | 通道是否在线、降级或离线 | UI 状态、离线事件 |
| 算法运行 | `ChannelRuntimeStatus.algorithm_status` | 算法是否运行、禁用或异常 | UI 状态、算法异常事件 |

通知只能由 `AlarmRecord` 状态变化触发，不直接由 `SceneState` 触发。这样可以避免简单模型一帧抖动导致重复通知。

### 5.4 报警状态机

```mermaid
stateDiagram-v2
    [*] --> normal
    normal --> suspected: 无人 + 亮灯达到疑似阈值
    suspected --> normal: 复核正常 / 灯灭 / 有人
    suspected --> alarm_active: 持续达到 alarm_hold_sec
    alarm_active --> acknowledged: 管理员确认
    alarm_active --> resolved: 达到恢复条件
    acknowledged --> resolved: 达到恢复条件
    resolved --> normal: 归档
```

### 5.5 通知发送

通知服务由报警状态变化触发，遵守以下规则：

- `alarm_active` 首次进入时发送 `alarm_triggered`。
- `resolved` 时按配置发送 `alarm_resolved`。
- 通知失败不影响本地报警、历史和 UI。
- 同一通道同一报警遵守冷却时间。
- 发送结果落入 `notification_history.json`。

---

## 6. 持久化设计

| 文件 | 内容 |
| --- | --- |
| `sources.json` | 视频源、分组 |
| `auth.json` | 管理员密码哈希 |
| `state_history.json` | 算法状态变化历史 |
| `algorithm_config.json` | 算法时段、周期、阈值、VLM 策略 |
| `roi_config.json` | ROI、阈值、标定样本 |
| `alarm_records.json` | 报警生命周期记录 |
| `runtime_status.json` | 可选：最近一次运行状态快照 |
| `notification_config.json` | 通知渠道、模板、冷却 |
| `notification_history.json` | 通知发送历史 |
| `security_config.json` | 外部 VLM、截图、隐私策略 |

所有配置文件应包含 `schema_version`，升级时执行迁移。

---

## 7. 关键技术决策

| 决策 | 结论 |
| --- | --- |
| 桌面框架 | Tauri 2 + Vite，降低包体并保留原生能力 |
| 存储方式 | 第一版使用本地 JSON，后续如查询压力增大再切 SQLite |
| 算法策略 | 简单模型高频，VLM 低频补漏 |
| 通知方式 | 先做 Webhook / HTTP POST，避免绑定具体 IM 平台 |
| 配置坐标 | ROI 使用归一化坐标 |
| 安全策略 | 外部 VLM 默认关闭，敏感字段不明文回显 |

ADR 记录见 [`decisions/`](./decisions/)。

---

## 8. 风险与约束

| 风险 | 影响 | 缓解 |
| --- | --- | --- |
| 灯光误检 | 误报警 | ROI、排除区、基线标定、持续时间 |
| 静止人员漏检 | 无人误判 | 轻量人形检测、多帧投票、VLM 复核 |
| VLM 成本过高 | 延迟和费用增加 | 有人跳过、低频、并发限制、小时上限 |
| RTSP 转码不稳定 | 视频不可看 | 明确 ffmpeg 进程管理、重启和日志 |
| JSON 配置增长 | 查询和迁移复杂 | schema_version、截断策略，必要时迁移 SQLite |
| 外部 VLM 隐私 | 截图外发风险 | 默认关闭、提示、打码、保留期配置 |
