## Why

用户要求将本轮图片投影修复及相关文档提交到 main，再核对 v2.4.1 发布准备，先不发布且不混入 MCP。

## What Changes

核对 main `565bbd24` 候选后，默认 CLI 编译发现 Pager 将仅 dev-dependency 的 sampling_types 用于生产路径。复用 Shell 已有生产 re-export 修正导入，不增加依赖或改变显示契约。开发指南明确独立检查默认生产构建和 all-targets，避免 dev-dependency 掩盖缺失引用。同时记录提交边界、原有回归证据、默认 CLI/工作区编译诊断、版本和文档一致性、发布门禁静态检查及未完成的正式验收。与已获授权的并行审计任务顺序提交，并由该任务统一推送 main；不改版本、不建 tag、不运行 release workflow。

## Scope

这是无行为变化的编译入口修复、发布分析与验证记录，使用 skip_specs，不新增 delta。代码和行为契约已由 hide-internal-image-projection-markers 归档负责；MCP 独立分支、未完成 change 和 stash 均不修改。
