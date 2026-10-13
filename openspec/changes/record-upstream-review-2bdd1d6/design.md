## Context

已有评审标记是历史文档章节 eb267fef..bc7f02ed，而本地 upstream/main 同为 bc7f02ed。前者是历史评审范围，后者是 Git 远端跟踪缓存，语义不同。ROADMAP.md 已迁为导航，未来方向应进入 openspec/backlog.md。

## Decisions

沿用已有文档入口，增加显式 reviewed target SHA、评审范围、未实现状态和 OpenSpec 链接。最近一次历史宽范围评审仍记录 bc7f02ed，新的2bdd目标仅表示本次两主题检查，不声称完整浏览其间所有提交。新审计细节只放本 change，不在 docs 新建需求清单。

Grow HEAD 事实与109条 working tree 在制修改分开记录。现有 backlog 已有用户修改，采用精确基线检查后追加一条，不替换其内容。主规范不写入未实现 delta。

Remote/Agent Host 仅保留未来跨设备方向，不展开架构/日期；MCP 与计划是已立项而未实现。排除项依用户决定写入两个 proposal 和评审表。

## Validation

校验三个新 change 及全量当前 OpenSpec；验证所有新本地链接、Requirement/Scenario 结构、任务勾选和源路径。写入前后核对原有变更文件内容 hash，确认新增仅指定 Markdown/YAML；不运行产品测试、不提交/推送。
