# ADR-0005 引入报警生命周期状态机

## 状态

已接受

## 背景

仅用 `alarm = !person && light` 无法表达疑似、确认、恢复、静默和通知冷却。实际使用中需要避免状态抖动和重复通知，也需要追溯管理员处理记录。

## 决策

引入报警状态机：

```text
normal -> vlm_checking -> suspected -> alarm_active -> acknowledged -> resolved -> normal
```

其中 `vlm_checking` 表示常规模型已连续 5 分钟无人且灯亮，系统正在等待 VLM 兜底确认。VLM 连续两次确认无人后进入 `suspected` 报警倒计时；倒计时达到 `alarm_hold_sec` 后进入 `alarm_active`，并发送通知。

## 理由

- 5 分钟人员保持和 `vlm_checking` 可以吸收算法抖动，避免立即通知。
- `alarm_active` 是正式报警和通知触发点。
- `acknowledged` 支持管理员确认和备注。
- `resolved` 支持恢复通知和闭环追溯。

## 后果

- 需要新增 `AlarmRecord`。
- UI 需要展示报警状态、确认入口和处理备注。
- UI 需要展示 VLM 确认进度和报警倒计时进度，例如报警图标双层边框进度环。
- 通知逻辑从简单布尔报警升级为状态变化触发。
