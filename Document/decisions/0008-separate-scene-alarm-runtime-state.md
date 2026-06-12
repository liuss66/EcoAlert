# ADR-0008 区分画面事实、业务报警和运行状态

## 状态

已接受

## 背景

`SceneState { person, light }` 表达算法看到的画面事实，但不能直接等同于业务报警。视频在线、算法异常、报警确认、通知冷却也不是画面事实的一部分。如果这些状态混在一起，UI 和通知逻辑会出现误判。

## 决策

系统明确区分四类状态：

- `SceneState`：画面事实。
- `AlarmRecord`：业务报警生命周期。
- `ChannelRuntimeStatus.online_status`：视频在线状态。
- `ChannelRuntimeStatus.algorithm_status`：算法运行状态。

通知只能由 `AlarmRecord` 状态变化触发，不直接由 `SceneState` 触发。

## 后果

- 需要新增运行状态模型。
- UI 需要分别展示在线、算法、报警状态。
- 报警状态机可以吸收算法抖动，减少重复通知。
- 视频离线和算法禁用不会被误解为“正常无报警”。

