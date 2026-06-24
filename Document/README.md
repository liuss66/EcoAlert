# Document

项目文档。开始编码前先来这里看 / 写。

## 阅读顺序

1. [`requirements.md`](./requirements.md)：确认产品目标、范围和验收标准。
2. [`architecture.md`](./architecture.md)：了解模块边界、数据流和状态模型。
3. [`api.md`](./api.md)：对齐前后端 commands、events 和通知 payload。
4. [`deployment.md`](./deployment.md)：查看开发、打包、数据目录和发布清单。
5. [`user-guide.md`](./user-guide.md)：面向最终用户的操作流程。
6. [`decisions/`](./decisions/)：需要理解关键取舍时查看 ADR。

## 目录

- `requirements.md` — 需求文档（功能 + 非功能 + 验收标准）
- `architecture.md` — 架构设计（模块图、数据流、关键决策）
- `api.md` — 前后端接口契约（Tauri commands / events）
- `deployment.md` — 打包 / 部署 / 升级
- `user-guide.md` — 软件使用流程与用户操作手册
- `changelog/` — 变更日志，按日期分文件
- `decisions/` — 关键决策记录（ADR 风格）

## 更新规则

- 需求、架构、接口、部署行为发生变化时，同步更新对应文档。
- 用户可见功能变化或发布前整理内容时，在 `changelog/` 新增或补充当天条目。
- 关键技术取舍需要新增 ADR，文件名按递增编号命名。
- 文档描述必须与当前可运行能力一致；计划中的能力要明确标注为“待实现”。
- 协议和版权信息以仓库根目录 [`../LICENSE`](../LICENSE) 为准。

## 当前文档基线

文档已于 2026-06-24 按当前工作区代码同步，覆盖 YOLO / VLM 真实接入、检测框、视频容器全屏、本地 MP4 播放进度同步以及当前报警复核流程。后续修改这些链路时，至少同步更新 `requirements.md`、`architecture.md`、`api.md`、`user-guide.md` 和当天变更日志。

## 写文档的约定

- 纯 Markdown，UTF-8
- 链接用相对路径，便于整个仓库移动
- 优先使用 Mermaid 图；只有确实需要图片时再新增 `assets/`
- 中文为主，代码 / 命令 / 路径保留英文
