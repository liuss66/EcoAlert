# EcoAlert 视频监控 · 需求规格说明书

> 版本：v1.8
> 日期：2026-06-13  
> 状态：实现中（前端 + Tauri 后端骨架完成，算法调度 / 报警闭环 / 通知发送骨架已接入，真实算法与配置 UI 待完善；分组拖拽已修复并优化性能）

---

## 1. 项目概述

### 1.1 背景

需要一个**本地可运行**的视频监控应用，用于查看实时视频流、对视频源进行管理、接收算法对每路视频的实时分析结果（人在场 / 灯亮状态）、在异常情况下发出报警和通知，并记录历史状态以供追溯。

### 1.2 目标

| 目标 | 衡量 |
| --- | --- |
| 支持本地视频源与远程推流 | 至少支持 HLS / MP4 / 摄像头 / RTSP 4 类 |
| 多人协作场景下视频源可分组管理 | 分组支持增删改、拖拽 |
| 算法可插拔 | 算法输出统一契约，前端不耦合具体实现 |
| 算法成本可控 | 简单模型高频巡检，VLM 低频补漏，按时段启停 |
| 报警可视化 | 无人 + 亮灯时报警，UI 突出显示 |
| 通知可配置 | 支持配置通知渠道、通知规则、冷却时间和测试发送 |
| 状态可追溯 | 每次状态变化都落库，能看时序 |
| 桌面应用形态 | Tauri 2，Windows / macOS / Linux |

### 1.3 非目标

- 不做云服务
- 不做录像存储（仅实时观看）
- 不做用户权限分级（单管理员密码）
- 不做复杂告警编排平台（仅提供本地通知配置、Webhook / HTTP 通知接口和必要冷却）
- 不做模型训练平台（只接入和调度已训练 / 已部署模型）

### 1.4 实施阶段

为避免第一版范围过大，后续实现按阶段推进：

| 阶段 | 范围 | 说明 |
| --- | --- | --- |
| MVP-1 | App 本体可用 | 登录、视频源 / 分组管理、MP4 / HLS 播放、状态历史、基础 UI |
| MVP-2 | 配置与运行状态 | 字段命名统一、配置文件框架、`ChannelRuntimeStatus`、算法调度器骨架 |
| MVP-3 | 报警闭环 | 报警状态机、确认 / 恢复、通知配置和通知历史骨架 |
| MVP-4 | 简单算法 | ROI 灯光规则、轻量人员检测接口、VLM mock provider |
| 后续 | 推流和真实模型 | ffmpeg 推流器已接入；RTSP 转码、真实 ONNX / VLM 接入待实现 |

当前目录结构也按这个边界收敛：`App/` 是主产品，`Video/` 是平铺测试素材，`Tools/` 是开发辅助工具；其中 HLS 推流器已可用于本地联调。

---

## 2. 用户角色

| 角色 | 能力 |
| --- | --- |
| **管理员** | 登录、CRUD 视频源、CRUD 分组、修改密码、查看所有数据 |
| **未登录用户** | 仅能看登录页 |

---

## 3. 功能需求

### 3.1 登录 / 鉴权

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-LOGIN-1 | 提供密码登录页，亮色主题，居中卡片 | ✅ |
| F-LOGIN-2 | 默认密码 `admin123`，首次启动写入 `auth.json`（SHA-256 + 盐） | ✅ |
| F-LOGIN-3 | 错误密码返回明确提示 | ✅ |
| F-LOGIN-4 | 登录后维持会话（内存态，重启失效） | ✅ |
| F-LOGIN-5 | 登录页有密码可见切换 | ✅ |
| F-LOGIN-6 | 登录后可在「系统设置」页修改密码 | ✅ |

### 3.2 视频管理

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-SRC-1 | 支持 4 种视频源类型：HLS、MP4、摄像头（webcam）、RTSP | ✅ |
| F-SRC-2 | 视频源可增 / 改 / 删 / 启停 | ✅ |
| F-SRC-3 | 视频源包含字段：名称、地址、类型、位置、是否启用、所属分组 | ✅ |
| F-SRC-4 | RTSP 需服务端转码（占位：浏览器显示"需转码"提示） | ⚠️ 占位 |
| F-SRC-5 | webcam 调浏览器 `getUserMedia` | ✅ |
| F-SRC-6 | 视频源数据持久化到 `%APPDATA%\com.ecoalert.monitor\sources.json` | ✅ |

### 3.3 分组

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-GRP-1 | 视频源可归属到某个分组 | ✅ |
| F-GRP-2 | 分组可增 / 改 / 删 | ✅ |
| F-GRP-3 | 分组内视频卡**横向**排布，到边界自动换行 | ✅ |
| F-GRP-4 | **分组之间垂直堆叠**（不横向并排） | ✅ |
| F-GRP-5 | 视频卡**可拖拽**到其他分组（HTML5 DnD）；拖放只移动被拖卡片，不重建整个网格（避免 HLS 流断开重连） | ✅ |
| F-GRP-6 | 分组头可折叠 / 展开 | ✅ |
| F-GRP-7 | 分组可重命名（点击铅笔图标，inline 编辑） | ✅ |
| F-GRP-8 | 默认分组不可删除 | ✅ |
| F-GRP-9 | 删除分组时，组内源自动回退到默认分组 | ✅ |
| F-GRP-10 | 视频管理页「新增视频源」modal 可选择所属分组 | ✅ |
| F-GRP-11 | 首次启动自动创建「默认分组」 | ✅ |

### 3.4 实时监控

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-LIVE-1 | 多路视频网格化展示 | ✅ |
| F-LIVE-2 | 视频卡结构：视频 → 名称/位置 → 状态图标行 → 操作按钮 | ✅ |
| F-LIVE-3 | 顶部状态 banner：当前报警数 + 涉及通道名 | ✅ |
| F-LIVE-4 | 视频卡悬浮「LIVE / 离线」标识 | ✅ |
| F-LIVE-5 | 后端每 3 秒推送一次码率 / FPS / 观众模拟数据 | ✅ |
| F-LIVE-6 | 后端每 4 秒推送一次算法场景状态（person / light） | ✅ |
| F-LIVE-7 | 算法真实接入时，替换 `state::spawn_scene_state_ticker` 即可 | ✅ 已接入 ffmpeg 单帧抽样，后续优化为常驻解码 |

### 3.5 状态图标（算法输出可视化）

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-ICN-1 | 视频卡**下方**固定一行展示算法输出 | ✅ |
| F-ICN-2 | 🧍 人：在场时显示，不在场时 `display: none` | ✅ |
| F-ICN-3 | 💡 灯：亮时显示，关时不显示 | ✅ |
| F-ICN-4 | 🚨 报警：仅当「无人 + 亮灯」时显示并红色闪烁 | ✅ |
| F-ICN-5 | 图标行**永远占位**（固定 34px），状态变化不引起卡片高度抖动 | ✅ |
| F-ICN-6 | 鼠标悬停 tooltip 解释当前状态 | ✅ |

### 3.6 监控总览

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-OV-1 | 5 张统计卡：视频源总数、在线、总码率、当前有人、报警中 | ✅ |
| F-OV-2 | 报警 banner（红色，列出报警通道） | ✅ |
| F-OV-3 | 「各通道状态」表：含在线 / 人 / 灯 / 报警 列 | ✅ |
| F-OV-4 | 「最近状态变更」表：来自 `state_history.json` 最近 50 条 | ✅ |

### 3.7 视频管理（CRUD 表格视图）

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-SRCT-1 | 表格列出所有视频源 | ✅ |
| F-SRCT-2 | 按名称 / 位置模糊搜索 | ✅ |
| F-SRCT-3 | 行内编辑 / 删除 | ✅ |

### 3.8 系统设置

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-SET-1 | 修改登录密码（当前密码 + 新密码 + 确认） | ✅ |
| F-SET-2 | 显示数据存储目录 | ✅ |

### 3.9 系统日志

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-LOG-1 | 后端事件实时推送到前端（info / warn / error） | ✅ |
| F-LOG-2 | 终端深色控制台样式 | ✅ |
| F-LOG-3 | 自动滚动开关 | ✅ |
| F-LOG-4 | 清空按钮 | ✅ |

### 3.10 算法接入契约

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-AI-1 | Rust 端定义 `SceneState { person, light, frame_seq, confidence, light_brightness, color_score, motion_score, process_ms }` | ✅ |
| F-AI-2 | 算法通过 `app.emit("ecoalert://scene_state", ...)` 推给前端 | ✅ 每次检测完成均推送 |
| F-AI-3 | 算法通过 `state.record_state_change(...)` 落库 | ✅ |
| F-AI-4 | 状态变化时记录一条 `StateRecord`（含派生 alarm 字段） | ✅ |
| F-AI-5 | `spawn_scene_state_ticker` 驱动开发期后台检测链路，后续可替换为常驻解码管线 | ✅ 已从合成帧推进到 ffmpeg 真实视频帧抽样 |
| F-AI-6 | 真实算法接入位置：`src-tauri/src/pipeline/detector.rs` 替换帧差法 | ⚠️ 灯光检测可用；人员仍是运动代理，轻量人形模型 / 常驻解码待实现 |

当前检测状态：

- 灯光：已接真实 RGB 抽帧，优先利用摄像头模式特征判断：开灯为彩色画面，关灯切红外后为黑白画面；算法计算 ROI 或全帧 `color_score` 并做 EMA + 磁滞阈值。RGB 不可用时才回退到 ROI 亮度 / 全帧亮度规则。
- 人员：尚未接入人形检测模型，`person` 由低分辨率帧差运动分数临时代理；适合观察“画面有明显变化”，不适合作为生产人员存在结论。
- 输出链路：Tauri 后端每次检测完成都会推送 `ecoalert://scene_state`；历史记录仍只在 `person / light` 变化时落库。
- 默认调度：全局 `activeWindows` 为空，表示全天运行；旧版内置的“工作日 18:30-08:30”默认窗口会在启动时迁移为空，避免演示视频长期停留在“等待结果”。
- 开发者模式：`developer_mode = true` 时忽略生效 / 例外时段，并且前端只在开发者模式下显示 `scene-readout` 检测读数。

### 3.11 算法策略与调度（新增）

#### 3.11.1 设计原则

本项目的核心报警条件是「无人 + 亮灯」，算法目标不是通用安防检测，而是尽量可靠地识别：

- 当前区域是否有人。
- 当前区域灯是否亮。
- 无人且灯亮是否持续到需要通知。

算法策略采用**简单模型 / 轻量规则高频巡检 + VLM 低频补漏**。简单模型承担主要实时判断，VLM 只在简单模型无法确认或疑似漏检时补充判断，避免每路视频频繁调用 VLM 导致成本、延迟和隐私风险增加。

#### 3.11.2 简单模型方案论证

| 识别目标 | 推荐方案 | 理由 | 风险 | 规避 |
| --- | --- | --- | --- | --- |
| 是否亮灯 | 彩色 / 红外黑白模式切换检测 + ROI 亮度兜底 | 快、稳定、无需训练，适配“开灯彩色、关灯红外黑白”的摄像头 | 摄像头未切红外、彩色噪声低、画面存在彩色屏幕时需调 ROI | 每路配置灯光 ROI、持续时间、必要时回退亮度阈值 |
| 是否有人 | 轻量 person detector（ONNX YOLOv8n / YOLO11n / RT-DETR 小模型）或动态目标识别 | 人是报警抑制条件，应优先减少误检；简单模型延迟低，可高频执行 | 静止人员、遮挡、远距离小目标漏检 | 多帧投票、置信度阈值、人员存在保持时间、VLM 低频补漏 |
| 动态目标识别 | 帧差 / 背景建模 / 光流作为辅助信号 | 计算量低，可用于发现画面变化和触发 VLM 复核 | 动态目标不等于人；风吹、反光、画面噪声容易误触发 | 只作为触发条件，不直接作为“有人”结论；Rust 侧已实现低分辨率帧差核心 |

结论：  
第一版不建议只用帧差法判断“有人”。帧差法适合作为快速任务识别 / 变化触发器，但“有人”应由轻量目标检测模型给出；灯光状态优先利用摄像头“彩色 / 红外黑白”切换特征完成，亮度阈值只作为兜底。这样能在成本可控的前提下尽可能避免误检。

#### 3.11.3 调度规则

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-AIS-1 | 每路视频可独立启用 / 停用算法 | ✅ 通道级算法配置 UI 已接入 |
| F-AIS-2 | 每路视频可配置算法启用时段，默认仅在下班后启用，例如 `18:30-08:30` | ✅ 全局 / 通道级时段 UI 已接入 |
| F-AIS-3 | 支持工作日 / 周末 / 节假日三类时段配置；第一版至少支持按星期配置 | ✅ 按星期配置已接入，跨天窗口按起始日归属 |
| F-AIS-4 | 简单模型高频执行，默认周期 5-15 秒，可按通道配置 | ✅ `simpleIntervalSec` 已接入后端真实抽帧周期 |
| F-AIS-5 | VLM 低频执行，默认周期 2-10 分钟，可按通道配置 | ⚠️ 全局 / 通道级 VLM 配置 UI 已接入，真实 VLM provider 待实现 |
| F-AIS-6 | 当简单模型识别到“有人”时，当前周期不得调用 VLM | ⚠️ 配置项已接入，真实 VLM 调度待实现 |
| F-AIS-7 | 当简单模型识别为“无人 + 亮灯”但置信度不足、状态刚变化、或持续时间达到复核阈值时，允许调用 VLM 复核 | ⏳ 待实现 |
| F-AIS-8 | VLM 主要用于修补简单模型漏检：确认是否有人、是否亮灯、是否存在遮挡 / 反光 / 日光干扰 | ⏳ 待实现 |
| F-AIS-9 | 简单模型与 VLM 输出需要保留来源字段，便于排查是 simple 还是 vlm 给出的结论 | ⏳ 待实现 |
| F-AIS-10 | 算法需支持冷却和并发限制，避免多路视频同时调用 VLM | ⏳ 待实现 |
| F-AIS-11 | 算法禁用时段内，不产生新报警、不调用模型；已存在报警可按配置自动恢复或保持展示 | ⏳ 待实现 |
| F-AIS-12 | 提供算法调度日志：跳过原因、模型耗时、置信度、VLM 调用次数 | ⚠️ 已推送跳过原因和调度事件，VLM 次数待接 |
| F-AIS-13 | 算法调度逻辑由独立 scheduler 模块负责，不放入 detector / analyzer | ⏳ 待实现 |
| F-AIS-14 | 配置支持继承：系统默认 < 全局配置 < 分组配置 < 通道配置 | ⚠️ 后端有效配置已支持；UI 已支持全局和通道级，分组级待接 |
| F-AIS-15 | UI 需要展示每项配置的来源层级，并支持恢复为继承值 | ⚠️ 已展示全局 / 通道 / 继承状态，并支持通道恢复全局继承 |

#### 3.11.4 推荐判定流程

```
[到达通道调度时间]
        │
        ▼
[当前是否在算法启用时段?] --否--> [跳过并记录 reason=schedule_disabled]
        │ 是
        ▼
[抽帧 + 预处理 + ROI 裁剪]
        │
        ▼
[简单模型 / 规则]
        │
        ├─ 有人? ──是──> [输出 person=true；不调用 VLM；报警恢复]
        │
        └─ 无人
              │
              ▼
        [灯亮?] --否--> [输出 normal；不调用 VLM]
              │ 是
              ▼
        [是否满足 VLM 复核条件?] --否--> [输出疑似/报警，等待持续时间]
              │ 是
              ▼
        [VLM 复核]
              │
              ▼
        [融合结果 + 冷却判断 + 通知]
```

#### 3.11.5 输出字段扩展

`SceneState` 保持向前兼容，新增字段建议如下：

```rust
pub struct SceneState {
    pub person: bool,
    pub light: bool,
    pub frame_seq: u64,
    pub confidence: f32,
    pub source: String,           // mock | simple | vlm | fused
    pub person_confidence: f32,
    pub light_confidence: f32,
    pub reason: Option<String>,   // schedule_disabled | simple_hit_person | vlm_recheck | ...
    pub model_latency_ms: Option<u32>,
}
```

### 3.12 通知接口与配置（新增）

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-NOTIFY-1 | 系统设置新增「通知配置」页面或分区 | ✅ 已接入 |
| F-NOTIFY-2 | 支持全局启用 / 停用通知 | ⏳ 待实现 |
| F-NOTIFY-3 | 支持 Webhook / HTTP POST 通知接口配置 | ✅ 已实现 |
| F-NOTIFY-4 | 通知配置包含：名称、渠道类型、URL、Method、Headers、Body 模板、超时、重试次数、启用状态 | ✅ 已实现 |
| F-NOTIFY-5 | 支持按事件类型选择通知：报警触发、报警恢复、视频离线、算法异常 | ⚠️ 已支持事件过滤，当前自动触发报警类事件 |
| F-NOTIFY-6 | 支持通知冷却时间，默认同一通道同一报警 30 分钟内不重复通知 | ✅ 已实现 |
| F-NOTIFY-7 | 支持测试发送，前端展示响应状态码和错误信息 | ✅ 已接入 |
| F-NOTIFY-8 | 通知发送结果写入系统日志，失败不影响本地报警展示 | ✅ 已实现 |
| F-NOTIFY-9 | 通知 payload 需要包含 source、location、person、light、alarm、ts、confidence、state_source | ✅ 已实现 |
| F-NOTIFY-10 | 敏感字段如 token / secret 在 UI 中默认脱敏显示 | ⏳ 待实现 |
| F-NOTIFY-11 | 支持飞书 / 企业微信 / QQ 渠道类型，除普通 Webhook 外可使用平台 API 凭证模式发送文本消息 | ⚠️ 后端已实现，UI 已有配置入口；生产联调待验证 |
| F-NOTIFY-12 | 支持渠道凭证校验，飞书支持本地 OAuth 扫码绑定并拉取群列表 | ⚠️ 后端命令已实现；当前 OAuth 仅支持飞书 |
| F-NOTIFY-13 | API 凭证模式自动获取、缓存并刷新 access token | ✅ 后端已实现 |

通知 payload 默认格式：

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
  "ts": 1781270400000
}
```

### 3.13 报警生命周期与抑制规则（新增）

报警状态必须从“当前画面状态”升级为可追踪的生命周期，避免同一问题反复通知、无法确认、无法恢复。

#### 3.13.1 状态机

```
normal
  │
  ├─ 无人 + 亮灯达到疑似阈值
  ▼
suspected
  │
  ├─ 持续达到 alarm_hold_sec
  ▼
alarm_active
  │
  ├─ 管理员确认
  ▼
acknowledged
  │
  ├─ 检测到有人 或 灯关闭 且持续达到 alarm_recover_sec
  ▼
resolved
  │
  └─ 归档后回到 normal
```

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-ALM-1 | 报警状态包含 `normal / suspected / alarm_active / acknowledged / resolved` | ⚠️ 运行态已支持 suspected；持久化记录支持 active / acknowledged / resolved |
| F-ALM-2 | `suspected` 状态仅本地展示，不发送外部通知 | ✅ 后端仅达到保持时间后才生成正式报警和通知 |
| F-ALM-3 | `alarm_active` 首次进入时发送 `alarm_triggered` 通知 | ✅ 后端已实现 |
| F-ALM-4 | 管理员可确认报警，确认后进入 `acknowledged`，记录确认人、确认时间、备注 | ✅ 后端与总览 UI 已实现 |
| F-ALM-5 | 已确认报警默认不重复发送触发通知，但仍可发送恢复通知 | ⚠️ 冷却已实现，确认态策略待细化 |
| F-ALM-6 | 报警恢复条件可配置：灯关闭、有人出现、二者任一、二者同时满足 | ✅ `recoverPolicy` 已接入后端状态机 |
| F-ALM-7 | 报警恢复需满足连续稳定时间，默认 60 秒，避免状态抖动 | ✅ `alarmRecoverSec` 已接入后端状态机 |
| F-ALM-8 | 支持对单通道临时静默，静默期间不发送通知但继续记录状态 | ⏳ 待实现 |
| F-ALM-9 | 支持例外日期 / 加班时段配置，例外时段内不触发“忘关灯”报警 | ⏳ 待实现 |
| F-ALM-10 | 视频离线作为独立事件，不应被误判为无人 + 亮灯 | ⏳ 待实现 |

### 3.14 ROI 与算法标定配置（新增）

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-ROI-1 | 每路视频可配置灯光检测 ROI，支持多个矩形区域 | ⚠️ 单灯光 ROI 配置 UI 已接入，默认全屏，多矩形待实现 |
| F-ROI-2 | 每路视频可配置排除 ROI，用于排除窗户、屏幕、反光区域 | ⏳ 待实现 |
| F-ROI-3 | 支持在视频画面上框选、拖拽、缩放 ROI | ⚠️ 当前支持 16:9 预览框内拖拽 / 缩放和数值配置，真实视频画面框选待实现 |
| F-ROI-4 | ROI 配置页面展示当前帧、ROI 覆盖层、实时亮度值和判定结果 | ⚠️ 当前展示 16:9 预览框；`test_roi_config` 已从当前视频源抽真实帧验证亮度 / 判定，页面实时帧预览待实现 |
| F-ROI-5 | 支持自动采样基线：记录“灯关”“灯亮”样本并生成推荐阈值 | ⏳ 待实现 |
| F-ROI-6 | 支持每路视频保存 `person_roi`，仅在有效区域内识别人 | ⏳ 待实现 |
| F-ROI-7 | ROI 坐标使用归一化坐标，避免不同分辨率下配置失效 | ⏳ 待实现 |
| F-ROI-8 | ROI 修改后产生配置版本，旧历史记录仍能追溯当时使用的版本 | ⏳ 待实现 |

### 3.15 算法验收指标（新增）

第一版验收目标分为 MVP 指标和生产目标指标。MVP 用于第一版上线验收，生产目标用于后续优化。

| 指标 | MVP 目标 | 生产目标 |
| --- | --- | --- |
| 灯亮识别延迟 | 60 秒内识别 | 30 秒内识别 |
| 灯光误报率 | 标定场景低于 10% | 标定场景低于 5% |
| 人员误检控制 | 有人时不触发正式报警，可进入 suspected | 有人时不进入正式报警，suspected 自动恢复 |
| VLM 调用抑制 | 简单模型识别到有人时，VLM 调用次数为 0 | 同 MVP |
| VLM 调用上限 | 每通道每小时不超过 12 次 | 每通道每小时不超过 6 次 |
| 报警通知延迟 | 正式报警后 30 秒内发起通知 | 正式报警后 10 秒内发起通知 |
| 通知失败隔离 | 通知失败不阻塞本地报警 | 同 MVP，并记录可重发历史 |
| 状态抖动控制 | 60 秒内不重复触发正式报警 | 可按通道调节抖动窗口 |

### 3.16 通知历史与重发（新增）

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-NH-1 | 所有通知发送结果落库，包含目标、事件、请求时间、响应状态、错误原因 | ✅ 后端已实现 |
| F-NH-2 | 通知历史支持按时间、通道、事件类型、成功 / 失败筛选 | ⚠️ 后端支持通道、事件类型、成功 / 失败、数量筛选 |
| F-NH-3 | 失败通知支持手动重发 | ✅ 后端已实现 |
| F-NH-4 | 自动重试达到上限后记录为失败，不再无限重试 | ⚠️ 单次发送失败记录已实现，自动重试队列待实现 |
| F-NH-5 | 通知模板变量需要在 UI 中列出并提供示例预览 | ⏳ 待实现 |

### 3.17 隐私与安全（新增）

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-SEC-1 | VLM 可配置本地模型或外部 API；外部 API 默认关闭 | ⏳ 待实现 |
| F-SEC-2 | 启用外部 VLM 前必须明确提示“截图可能发送到第三方服务” | ⏳ 待实现 |
| F-SEC-3 | VLM 截图默认不落盘；如开启保存，必须配置保留天数 | ⏳ 待实现 |
| F-SEC-4 | 通知 payload 默认不包含图片，仅包含结构化状态 | ⏳ 待实现 |
| F-SEC-5 | API Key、Webhook Token、Header Secret 等敏感字段需加密或系统安全存储 | ⏳ 待实现 |
| F-SEC-6 | UI 展示敏感字段时默认脱敏，仅允许重新输入，不明文回显 | ⏳ 待实现 |
| F-SEC-7 | 支持可选人脸 / 人体区域打码后再发送给 VLM 或通知渠道 | ⏳ 待实现 |
| F-SEC-8 | 配置合理的 CSP 内容安全策略，至少限制 `script-src` 和 `connect-src`，避免 `csp: null` 完全关闭 | ⏳ 待实现 |
| F-SEC-9 | 密码哈希升级为 argon2id（或至少 PBKDF2），替换当前 SHA-256 + salt 方案，提高暴力破解抵抗力 | ⏳ 待实现 |
| F-SEC-10 | 登录接口增加速率限制：连续 N 次（建议 5 次）失败后临时锁定账户（建议 5 分钟），并记录安全日志 | ⏳ 待实现 |
| F-SEC-11 | 会话管理增加超时机制（建议 30 分钟无操作自动失效），避免当前 `logged_in` 布尔值永久有效 | ⏳ 待实现 |

### 3.18 配置导入导出与迁移（新增）

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-CFG-1 | 支持导出全部配置：视频源、分组、算法配置、ROI、通知配置 | ⏳ 待实现 |
| F-CFG-2 | 支持导入配置，并在导入前展示差异和冲突 | ⏳ 待实现 |
| F-CFG-3 | 支持重置为默认配置，但不得删除历史记录，除非用户明确选择 | ⏳ 待实现 |
| F-CFG-4 | 配置文件包含 schema_version，升级时自动迁移旧版本配置 | ⏳ 待实现 |
| F-CFG-5 | 导出配置时默认不包含敏感密钥；如包含必须二次确认 | ⏳ 待实现 |
| F-CFG-6 | 导入同 ID 配置时默认覆盖，用户可选择生成新 ID | ⏳ 待实现 |
| F-CFG-7 | 导入视频源 URL 相同但 ID 不同时，标记为潜在重复并要求用户确认 | ⏳ 待实现 |
| F-CFG-8 | 导入通知配置缺失密钥时，默认禁用该通知目标并提示补全 | ⏳ 待实现 |
| F-CFG-9 | 导入默认不包含历史记录；历史记录导入必须单独选择 | ⏳ 待实现 |

### 3.19 运行状态模型（新增）

系统需要区分“画面事实”“业务报警”“视频在线”“算法运行”四类状态，避免 UI 将“没有报警”误解为“一切正常”。

| 编号 | 需求 | 状态 |
| --- | --- | --- |
| F-STAT-1 | `SceneState` 只表达算法看到的画面事实：人、灯、置信度、来源 | ⏳ 待实现 |
| F-STAT-2 | `AlarmRecord` 只表达业务报警生命周期，不直接等同于 `SceneState.alarm` | ✅ 骨架已实现 |
| F-STAT-3 | 通知只能由 `AlarmRecord` 状态变化触发，不直接由 `SceneState` 触发 | ✅ 骨架已实现 |
| F-STAT-4 | 视频源需要独立 `online_status`：`online / offline / degraded` | ⏳ 待实现 |
| F-STAT-5 | 算法需要独立 `algorithm_status`：`idle / running / disabled / error` | ⏳ 待实现 |
| F-STAT-6 | 每路视频记录 `last_frame_at`、`last_algorithm_at`、`last_error` | ⏳ 待实现 |
| F-STAT-7 | UI 同时展示视频在线状态、算法状态和报警状态 | ⚠️ 报警记录 UI 已接入，运行状态细节展示待增强 |

---

## 4. 数据模型

### 4.1 VideoSource

```rust
pub struct VideoSource {
    pub id: String,                // src-<uuid>
    pub name: String,              // ≤ 64
    pub url: String,               // ≤ 512
    pub source_type: String,       // hls | mp4 | webcam | rtsp
    pub location: String,          // ≤ 128
    pub enabled: bool,
    pub group_id: Option<String>,
    pub order: i32,
    pub created_at: i64,           // ms
}
```

### 4.2 SourceGroup

```rust
pub struct SourceGroup {
    pub id: String,                // grp-<uuid>，默认分组为 grp-default
    pub name: String,              // ≤ 64
    pub order: i32,
    pub collapsed: bool,
    pub created_at: i64,
}
```

### 4.3 SceneState（算法实时输出）

```rust
pub struct SceneState {
    pub person: bool,
    pub light: bool,
    pub frame_seq: u64,
    pub confidence: f32,
}
```

### 4.4 StateRecord（历史记录）

```rust
pub struct StateRecord {
    pub id: String,                // rec-<uuid>
    pub source_id: String,
    pub person: bool,
    pub light: bool,
    pub alarm: bool,               // = !person && light
    pub ts: i64,                   // ms
}
```

存储位置：`%APPDATA%\com.ecoalert.monitor\state_history.json`  
截断策略：保留最近 5000 条。

### 4.5 AlgorithmConfig（新增）

```rust
pub struct AlgorithmConfig {
    pub enabled: bool,
    pub developer_mode: bool,              // true 时忽略生效时段，并显示调试读数
    pub scope: String,                    // system | global | group | source
    pub scope_id: Option<String>,
    pub source_id: Option<String>,        // None 表示全局默认
    pub active_windows: Vec<ActiveWindow>,
    pub exception_windows: Vec<ActiveWindow>,
    pub simple_interval_sec: u32,         // 默认 10
    pub vlm_interval_sec: u32,            // 默认 300
    pub vlm_enabled: bool,
    pub vlm_skip_when_person: bool,       // 默认 true
    pub person_threshold: f32,
    pub light_threshold: f32,
    pub alarm_hold_sec: u32,              // 持续多久才报警
    pub alarm_recover_sec: u32,           // 持续多久才恢复
    pub recover_policy: String,           // light_off | person_present | either | both
    pub vlm_hourly_limit: u32,
    pub roi_version: Option<String>,
}
```

配置合并优先级：`system < global < group < source`。后一级只覆盖显式设置的字段，未设置字段继承前一级。

### 4.6 ActiveWindow（新增）

```rust
pub struct ActiveWindow {
    pub weekdays: Vec<u8>,                // 1..7，周一到周日
    pub start: String,                    // HH:mm
    pub end: String,                      // HH:mm，允许跨天
    pub timezone: String,                 // 默认本机时区
}
```

示例：周一至周五 `18:30-08:30`，周末全天启用。

### 4.7 NotificationTarget（新增）

```rust
pub struct NotificationTarget {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub channel_type: String,             // webhook | feishu | wechat_work | qqbot
    pub url: String,
    pub method: String,                   // POST | PUT
    pub headers: Vec<HeaderPair>,
    pub body_template: String,
    pub timeout_sec: u32,
    pub retry_count: u32,
    pub event_types: Vec<String>,         // alarm_triggered | alarm_resolved | source_offline | algorithm_error
    pub cooldown_sec: u32,
    pub created_at: i64,
    pub app_id: String,                   // 飞书 App ID / 企微 CorpID / QQ AppID
    pub app_secret: String,               // 飞书 App Secret / 企微 Secret / QQ ClientSecret
    pub agent_id: String,                 // 企微 AgentID
    pub chat_id: String,                  // 飞书 chat_id / 企微 touser / QQ group_openid
    pub access_token: String,             // 后端缓存
    pub token_expires_at: i64,            // 秒级时间戳
}
```

`channel_type = webhook` 时使用原始 URL / Header / 模板发送；`feishu`、`wechat_work`、`qqbot` 且配置了 `app_id` 时走平台 API 凭证模式，自动刷新 token。飞书额外提供本地 OAuth 扫码绑定命令以获取群列表。

### 4.8 RoiConfig（新增）

```rust
pub struct RoiConfig {
    pub id: String,
    pub source_id: String,
    pub version: String,
    pub light_rois: Vec<RoiRect>,
    pub exclude_rois: Vec<RoiRect>,
    pub person_rois: Vec<RoiRect>,
    pub light_on_threshold: f32,
    pub light_off_threshold: f32,
    pub baseline_samples: Vec<BaselineSample>,
    pub updated_at: i64,
}

pub struct RoiRect {
    pub id: String,
    pub label: String,
    pub x: f32,                           // 0..1
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
```

### 4.9 AlarmRecord（新增）

```rust
pub struct AlarmRecord {
    pub id: String,
    pub source_id: String,
    pub status: String,                   // suspected | alarm_active | acknowledged | resolved
    pub first_seen_at: i64,
    pub triggered_at: Option<i64>,
    pub acknowledged_at: Option<i64>,
    pub resolved_at: Option<i64>,
    pub acknowledged_by: Option<String>,
    pub note: Option<String>,
    pub last_state_id: Option<String>,
}
```

### 4.10 NotificationRecord（新增）

```rust
pub struct NotificationRecord {
    pub id: String,
    pub target_id: String,
    pub event: String,
    pub source_id: Option<String>,
    pub alarm_id: Option<String>,
    pub ok: bool,
    pub status_code: Option<u16>,
    pub error: Option<String>,
    pub request_at: i64,
    pub latency_ms: Option<u32>,
    pub retry_count: u32,
}
```

### 4.11 SecurityConfig（新增）

```rust
pub struct SecurityConfig {
    pub external_vlm_enabled: bool,
    pub save_vlm_snapshots: bool,
    pub snapshot_retention_days: u32,
    pub include_image_in_notification: bool,
    pub blur_person_before_external_send: bool,
}
```

### 4.12 ChannelRuntimeStatus（新增）

```rust
pub struct ChannelRuntimeStatus {
    pub source_id: String,
    pub online_status: String,            // online | offline | degraded
    pub algorithm_status: String,         // idle | running | disabled | error
    pub alarm_status: String,             // normal | suspected | alarm_active | acknowledged | resolved
    pub last_frame_at: Option<i64>,
    pub last_algorithm_at: Option<i64>,
    pub last_error: Option<String>,
    pub effective_algorithm_config_scope: String,
    pub ts: i64,
}
```

新增存储文件：
- `algorithm_config.json` — 全局与通道级算法配置
- `notification_config.json` — 通知渠道、模板和冷却配置
- `roi_config.json` — 通道 ROI、阈值和标定样本
- `alarm_records.json` — 报警生命周期与确认记录
- `notification_history.json` — 通知发送历史
- `security_config.json` — VLM 外发、截图保存和隐私策略

---

## 5. 接口契约（Tauri Commands / Events）

### 5.1 Commands

| 名称 | 参数 | 返回 | 鉴权 |
| --- | --- | --- | --- |
| `login` | `password: String` | `{ ok, token }` | — |
| `logout` | — | `{ ok }` | ✓ |
| `check_auth` | — | `{ ok }` | — |
| `list_sources` | — | `VideoSource[]` | ✓ |
| `list_groups` | — | `SourceGroup[]` | ✓ |
| `create_source` | `payload: SourcePayload` | `VideoSource` | ✓ |
| `update_source` | `id, payload` | `VideoSource` | ✓ |
| `delete_source` | `id` | `{ ok }` | ✓ |
| `create_group` | `payload: GroupPayload` | `SourceGroup` | ✓ |
| `update_group` | `id, payload` | `SourceGroup` | ✓ |
| `delete_group` | `id` | `{ ok }` | ✓ |
| `reorder` | `items: OrderItem[]` | `{ ok }` | ✓ |
| `report_scene_state` | `source_id, person, light` | `{ ok }` | ✓ |
| `get_state_history` | `source_id?, limit?` | `{ ok, records, by_source }` | ✓ |
| `get_algorithm_config` | `source_id?` | `AlgorithmConfig` | ✓ |
| `update_algorithm_config` | `source_id?, payload` | `AlgorithmConfig` | ✓ |
| `delete_algorithm_config` | `source_id` | `{ ok }` | ✓ |
| `get_effective_algorithm_config` | `source_id` | `{ config, sources }` | ✓ |
| `get_roi_config` | `source_id` | `RoiConfig` | ✓ |
| `update_roi_config` | `source_id, payload` | `RoiConfig` | ✓ |
| `test_roi_config` | `source_id, payload?` | `{ ok, light, person, brightness, motionScore, confidence, processMs, version }` | ✓ |
| `list_alarms` | `status?, source_id?, limit?` | `AlarmRecord[]` | ✓ |
| `get_channel_runtime_status` | `source_id?` | `ChannelRuntimeStatus[]` | ✓ |
| `ack_alarm` | `alarm_id, note?` | `AlarmRecord` | ✓ |
| `resolve_alarm` | `alarm_id, note?` | `AlarmRecord` | ✓ |
| `mute_source` | `source_id, until_ts, reason?` | `{ ok }` | ⏳ 未注册 |
| `list_notification_targets` | — | `NotificationTarget[]` | ✓ |
| `create_notification_target` | `payload` | `NotificationTarget` | ✓ |
| `update_notification_target` | `id, payload` | `NotificationTarget` | ✓ |
| `delete_notification_target` | `id` | `{ ok }` | ✓ |
| `test_notification_target` | `id? 或 payload` | `NotificationRecord` | ✓ |
| `list_notification_history` | `source_id?, event?, ok?, limit?` | `NotificationRecord[]` | ✓ |
| `resend_notification` | `record_id` | `NotificationRecord` | ✓ |
| `get_security_config` | — | `SecurityConfig` | ✓ |
| `update_security_config` | `payload` | `SecurityConfig` | ✓ |
| `start_oauth_binding` | `channel_type, app_id, app_secret` | `{ sessionId, port, authUrl, qrData }` | ✓ |
| `check_oauth_status` | `session_id, app_id, app_secret` | `{ status }` 或 `{ status, accessToken, chats }` | ✓ |
| `verify_channel_credentials` | `channel_type, app_id, app_secret` | `{ ok, message }` | ✓ |
| `export_config` | `include_secrets: bool` | `String 或文件路径` | ⏳ 未注册 |
| `import_config` | `payload, dry_run: bool` | `{ ok, diff, warnings }` | ⏳ 未注册 |
| `change_password` | `old_password, new_password` | `{ ok }` | ✓ |
| `get_data_dir` | — | `String` | ✓ |

### 5.2 Events

| 事件 | Payload | 频率 |
| --- | --- | --- |
| `ecoalert://event` | `{ level, text, ts }` | 不定 |
| `ecoalert://status` | `ChannelStatus[]` | 3s |
| `ecoalert://runtime_status` | `ChannelRuntimeStatus[]` | 3s 或状态变化时 |
| `ecoalert://sources` | `VideoSource[]` | 源变更时 |
| `ecoalert://scene_state` | `{ source_id, person, light, alarm, alarm_status, ts }` | 状态变化或心跳 |
| `ecoalert://alarm` | `{ alarm_id, source_id, status, event, ts }` | 报警状态变化时 |
| `ecoalert://notification` | `{ target_id, event, ok, status, error, ts }` | 通知发送后 |
| `ecoalert://algorithm_schedule` | `{ source_id, action, reason, latency_ms, ts }` | 算法调度时 |

---

## 6. UI 规范

### 6.1 主题

- 亮色主题，蓝紫渐变 LOGO（`#3b82f6` → `#8b5cf6`）
- 圆角统一 `--radius: 10px`，卡片 `--radius-lg: 14px`
- 阴影两级：sm（hover 时），lg（弹窗）
- 主色蓝 `--primary: #3b82f6`，成功绿、警告琥珀、报警红

### 6.2 布局

| 区域 | 规则 |
| --- | --- |
| 侧边栏 | 固定 232px，含 LOGO / 5 个导航 / WebSocket 状态 / 退出 |
| 顶栏 | 64px，含视图标题 + 实时时钟 + 用户芯片 |
| 主区 | padding 22 / 24 |
| 视频网格（实时监控） | 4 个分组**垂直堆叠**，每组卡片**横向 grid**（`auto-fill, minmax(220px, 1fr)`） |
| 视频卡 | 16:9 视频区 + meta + 固定 34px 状态图标行 + 操作 |

### 6.3 交互

- 拖拽：HTML5 DnD，被拖卡片半透明；目标分组显示蓝色边框 + 浅蓝底（含已有卡片的分组整体高亮）；源分组表头半透明
- 模态：覆盖 + 缩放进入动画
- 报警：banner 红色，报警图标 0.9s 闪烁

---

## 7. 数据流

```
当前开发期：

Video/*.mp4
        │
        ▼
Tools/push_streamer 生成 HLS
        │
        ▼
webui 播放预览
        │
        ▼
Tauri 后端 ffmpeg 单帧抽样 + Detector
        │
        ▼
实时检测读数 / 图标 / 报警 banner / 状态历史

目标生产链路：

Video / Camera / RTSP / HLS
        │
        ▼
stream/  (HLS / RTMP / RTSP 转码)
        │  FramePacket
        ▼
pipeline/  (scheduler → decoder → detector → analyzer → alerts)
        │  SceneState / AlarmRecord
        ▼
state.rs  (record_state_change + app.emit)
        │                    │
        ▼                    ▼
state_history.json    ecoalert://scene_state / ecoalert://alarm
                             │
                             ▼
                           webui
```

---

## 8. 部署与运行

### 8.1 浏览器预览（开发期，不依赖 Rust）

```bash
cd App
npm install
npm run dev          # http://localhost:1420
```

前端自动降级到 localStorage 模拟视频源、分组和配置；浏览器预览模式不再伪造 person / light 检测结果，真实检测事件只在 Tauri 模式由后端产生。

### 8.2 Tauri 桌面应用（生产）

```bash
cd App
npm install
npm run tauri:dev     # 开发
npm run tauri:build   # 打包 .msi / .exe / .dmg
```

### 8.3 数据存储位置

| OS | 路径 |
| --- | --- |
| Windows | `%APPDATA%\com.ecoalert.monitor\` |
| macOS | `~/Library/Application Support/com.ecoalert.monitor/` |
| Linux | `~/.local/share/com.ecoalert.monitor/` |

文件：
- `sources.json` — 视频源 + 分组
- `auth.json` — 密码哈希
- `state_history.json` — 状态历史
- `algorithm_config.json` — 算法启用时段、模型周期、阈值、VLM 调度
- `notification_config.json` — 通知渠道、模板、冷却和重试配置
- `roi_config.json` — ROI、阈值和标定样本
- `alarm_records.json` — 报警状态机、确认和恢复记录
- `notification_history.json` — 通知发送历史
- `security_config.json` — 隐私和外部 VLM 策略

### 8.4 系统要求

- **Node.js 18+**
- **Rust 1.77+**（仅 Tauri 模式）
- **WebView2**（Windows 10+ / 11 自带）
- **ffmpeg**（当前 HLS 推流器和 Tauri 后端单帧抽样依赖；RTSP 转码后续继续依赖）

---

## 9. 验收用例

| 编号 | 场景 | 期望结果 |
| --- | --- | --- |
| AC-1 | 下班时段，无人 + 灯亮持续达到 `alarm_hold_sec` | 进入 `alarm_active`，UI 报警，发送一次 `alarm_triggered` 通知 |
| AC-2 | 下班时段，有人 + 灯亮 | 不触发正式报警，不调用 VLM |
| AC-3 | 下班时段，无人 + 灯灭 | 状态正常，不报警，不通知 |
| AC-4 | 上班时段，即使无人 + 灯亮 | 算法跳过或不产生报警，记录 `schedule_disabled` |
| AC-5 | 简单模型判断无人 + 亮灯，但置信度不足 | 进入 `suspected`，按配置触发 VLM 复核，不立即通知 |
| AC-6 | VLM 复核发现有人 | 恢复正常或保持非报警状态，不发送触发通知 |
| AC-7 | 同一通道报警已发送，冷却期内状态仍为报警 | 不重复发送同类通知 |
| AC-8 | 管理员确认报警 | 状态变为 `acknowledged`，记录确认时间和备注 |
| AC-9 | 已确认报警恢复 | 状态变为 `resolved`，按配置发送 `alarm_resolved` 通知 |
| AC-10 | 通知接口超时或返回 500 | 本地报警和历史正常记录，通知历史记录失败原因 |
| AC-11 | 视频源离线 | 产生 `source_offline` 事件，不被判定为无人 + 亮灯 |
| AC-12 | 修改 ROI 后测试 | UI 展示亮度值、判定结果和使用的 ROI 版本 |
| AC-13 | 外部 VLM 未启用 | 系统不得向外部 API 发送截图 |
| AC-14 | 导入配置 dry-run | 展示差异和冲突，不修改当前配置 |

---

## 10. 项目优化建议

### 10.1 产品与需求优化

| 优先级 | 优化项 | 说明 |
| --- | --- | --- |
| 高 | 明确报警闭环 | 当前已有 UI 报警，但缺少通知、恢复、确认、冷却和处理记录；应先补齐“触发-通知-恢复-追溯”闭环 |
| 高 | 算法配置产品化 | 算法启用时段、阈值、ROI、周期、VLM 调用策略需要进入配置页面，不能硬编码 |
| 高 | 报警误检治理 | 灯光识别需支持 ROI 和持续时间；人员识别需支持多帧投票和恢复延迟 |
| 中 | 状态历史查询增强 | 支持按通道、时间范围、报警状态筛选和导出 |
| 中 | 分组 / 通道批量配置 | 多路摄像头场景下，需要批量设置算法启用时段、通知规则、阈值 |
| 中 | 告警确认与备注 | 管理员可确认告警、填写处理备注，形成审计记录 |

### 10.2 工程优化

| 优先级 | 优化项 | 说明 |
| --- | --- | --- |
| 高 | 前后端字段命名统一 | Rust 使用 `source_type / group_id / created_at`，前端使用 `type / groupId / createdAt`，需要统一 serde rename 或 API 转换层 |
| ~~高~~ ✅ | mock 与真实数据一致性 | ~~浏览器 mock 每次返回默认数据，新增 / 修改不能稳定复现~~；已修复：`mockLoad` / `mockLoadGroups` 现优先读 localStorage，首次才写入默认值 |
| ~~高~~ ✅ | 算法 pipeline 接入状态推送 | 已由 `spawn_scene_state_ticker` 调度 ffmpeg 单帧抽样 + `Detector::analyze_scene()`，并推送 `scene_state / alarm / algorithm_schedule`；后续优化为常驻解码管线 |
| 中 | RTSP / HLS 转码闭环 | 文档写 RTSP 支持，但实现仍是占位提示；需要明确依赖 ffmpeg 还是内置转码服务 |
| 中 | 日志持久化 | 当前系统日志主要是前端内存态，重启后不可查；建议后端落盘并支持查询 |
| 中 | 配置 schema 与迁移 | 新增算法 / 通知配置后，需要版本号、默认值和旧配置迁移 |
| 中 | 测试补齐 | 至少补 Tauri command 单测、配置读写单测、通知模板渲染单测、算法调度规则单测 |
| 低 | 大文件治理 | `Video/*.mp4` 体积较大，不建议提交到主仓；可改为外部下载或 Git LFS |
| ~~高~~ ✅ | 文件持久化原子写入 | ~~使用「写临时文件 + rename」模式，防止写入中途崩溃导致数据损坏~~；已接入 `write_temp_then_replace` |
| 高 | 锁竞争风险治理 | `AppState` 有 11 个 `Mutex`，在 `spawn_scene_state_ticker` 等函数中嵌套获取存在死锁风险；建议合并为更少的 `RwLock` 或重新设计锁粒度 |
| ~~高~~ ✅ | reorder 命令日志 bug | 已改为先记录 `items.len()`，日志输出真实重排数量 |
| 中 | 前端模块化拆分 | `main.js` 1300+ 行单文件，建议拆为 `auth` / `live` / `overview` / `settings` / `notifications` 五个模块 |
| 中 | 前端增量渲染优化 | `renderOverview()` 每 4s 全量重建表格 DOM，视频源多时性能差；建议差量更新或节流 |
| 中 | 通知目标编辑功能 | 前端通知表单只支持新建（始终调 `createNotificationTarget`），缺少编辑已有通知目标的逻辑 |
| ~~中~~ ✅ | reqwest Client 复用 | 通知发送和渠道 token 获取均已使用 `OnceLock<reqwest::Client>` 复用 HTTP client |
| 中 | 错误处理增强 | 多处静默吞错（前端 `catch (_) {}`、后端 `let _ = save()`），应至少记录 warn 日志 |
| ~~低~~ ✅ | 历史记录容器优化 | 状态历史、报警记录、通知历史已改用 `VecDeque`，JSON 仍兼容数组格式 |
| 低 | 结构化日志升级 | 从 `env_logger` 迁移到 `tracing`，支持 `source_id` / `alarm_id` 等结构化字段，便于生产环境排错 |

## 11. 未来工作

| 优先级 | 任务 |
| --- | --- |
| 高 | 真实人员识别接入：轻量人形检测 + VLM 低频复核 |
| 高 | ROI 标定增强：真实画面框选、多 ROI、排除区、基线采样和版本管理 |
| 高 | 算法配置增强：分组级覆盖、VLM 调度策略、并发限制和调用上限落地 |
| 高 | 报警生命周期增强：确认态策略、静默、例外时段、离线事件隔离 |
| 高 | 通知配置完善：全局启停、敏感字段脱敏、模板变量预览 |
| 高 | 通知历史、失败重发和通知模板变量预览 |
| 高 | RTSP → HLS 转码服务（`stream/rtmp.rs`） |
| 中 | 状态历史按时间段筛选 / 导出 |
| 中 | 隐私安全配置：外部 VLM、截图保存、密钥存储、打码 |
| 中 | 配置导入导出、dry-run、schema 迁移 |
| 中 | 多用户与权限分级 |
| 低 | 录像 / 截图 / 时间轴回放 |
| 低 | 移动端 / Web 端适配 |

---

## 12. 变更记录

| 日期 | 版本 | 变更 |
| --- | --- | --- |
| 2026-06-13 | 0.1 | 初稿，Node + Express 起步 |
| 2026-06-13 | 0.5 | 切换到 Tauri 2 + Vite 架构 |
| 2026-06-13 | 0.6 | 加入 pipeline/stream 模块骨架 |
| 2026-06-13 | 0.7 | 加入分组 + 拖拽 + 状态图标 + 历史 |
| 2026-06-13 | 1.0 | UI 收敛：状态图标用存在性表达，固定行高，分组垂直堆 |
| 2026-06-13 | 1.1 | 补充简单模型 + VLM 分层算法方案、算法启用时段、通知接口 / 配置能力和项目优化建议 |
| 2026-06-13 | 1.2 | 补充报警生命周期、ROI 标定、算法验收指标、通知历史、隐私安全、配置导入导出和验收用例 |
| 2026-06-13 | 1.3 | 修复分组拖拽：去除 `dragstart` 对 `<video>` 的误拦截（视频区占卡片 70-80% 面积，之前完全拖不动）；修复 `mockLoad` / `mockLoadGroups` 永远返回默认数据的 bug（浏览器预览模式拖放后卡片回弹）；拖放后只 `appendChild` 被拖卡片，不再 `renderLive()` 重建整个网格（避免 HLS 流全部断开重连导致卡顿）；增强拖拽视觉反馈（目标分组整体高亮、源分组表头半透明） |
| 2026-06-13 | 1.4 | 补充安全需求（CSP / 密码哈希升级 / 登录防暴力破解 / 会话超时）；补充工程优化项（原子写入 / 锁竞争治理 / 前端模块化 / 增量渲染 / 通知目标编辑 / reqwest Client 复用 / 错误处理增强 / 结构化日志 / reorder 日志 bug 修复 / VecDeque 优化） |
| 2026-06-13 | 1.5 | Rust 侧移植简单视觉检测核心并接入开发期运行链路：ROI 灯光亮度 + 磁滞阈值 + EMA 平滑、低分辨率帧差运动识别、合成帧单元测试；后台 ticker 当前使用合成灰度帧驱动 `Detector::analyze_scene()`，后续只替换真实帧源 |
| 2026-06-13 | 1.6 | 系统设置新增 ROI 标定最小闭环：按通道选择、单灯光 ROI 归一化坐标、亮/灭灯阈值、16:9 可拖拽 / 缩放预览框、保存到 `roi_config.json`；实现 `test_roi_config`，支持用合成标定帧测试亮度 / 判定；浏览器 mock 模式支持 localStorage 持久化 |
| 2026-06-13 | 1.7 | 根据当前代码同步通知渠道能力：补充 `webhook / feishu / wechat_work / qqbot` 渠道类型、平台 API 凭证字段、access token 缓存、飞书 OAuth 扫码绑定和凭证校验 commands；修正未注册的 `mute_source / export_config / import_config` 状态 |
| 2026-06-13 | 1.8 | 根据当前代码同步真实抽帧与报警状态机进展：后台检测改为 ffmpeg 单帧抽样，浏览器预览停止伪造算法结果，`scene_state` 增加 `alarm / alarm_status`，`simpleIntervalSec / alarmHoldSec / alarmRecoverSec / recoverPolicy` 已接入后端运行链路；Tools 推流器和测试可视化视频生成已补齐 |
