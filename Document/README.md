# Document

项目文档。开始编码前先来这里看 / 写。

## 目录

- `requirements.md` — 需求文档（功能 + 非功能 + 验收标准）
- `architecture.md` — 架构设计（模块图、数据流、关键决策）
- `api.md` — 前后端接口契约（Tauri commands / events）
- `deployment.md` — 打包 / 部署 / 升级
- `user-guide.md` — 软件使用流程与用户操作手册
- `changelog/` — 变更日志，按日期分文件
- `decisions/` — 关键决策记录（ADR 风格）

## 写文档的约定

- 纯 Markdown，UTF-8
- 链接用相对路径，便于整个仓库移动
- 优先使用 Mermaid 图；只有确实需要图片时再新增 `assets/`
- 中文为主，代码 / 命令 / 路径保留英文
