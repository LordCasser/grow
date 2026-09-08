## Context

迁移开始于 main，HEAD 比 origin/main 超前一个提交，工作树存在用户未提交修改。本次创建 codex/openspec-sdd，保留原状态。基线必须反映实际读取的实现，不能标成已发布版本。

## Goals / Non-Goals

建立能力契约、代码证据、开发流程与 docs 分工。不做业务重构、不实施历史路线图、不把格式校验当行为测试。

## Decisions

1. 使用官方 spec-driven schema 和 proposal → specs/design → tasks → apply/verify → archive 流程。基线也通过 ADDED delta 归档，避免特殊旁路。
2. 按可观察能力分组，不按 57 个 workspace crate 各建一份 spec；辅助 crate 的职责通过 baseline 模块图追溯。
3. 规范使用中文和标准英文结构。每项要求附代码路径与符号，源码哈希保存在本次归档 evidence.json；它是历史证据，不要求以后代码哈希保持不变。
4. docs 保留阅读材料，历史审计明确冻结；新过程记录只进入 change。ROADMAP 内容迁入 backlog，原路径留下指引。
5. 根 AGENTS.md 是工具无关执行入口，使用 CLI 操作。此次 init 使用 --tools none，不依赖被 gitignore 排除的个人 .codex 配置或假设 slash command 已安装。
6. CI 校验 specs、active changes 和 archive 任务状态。它只保证 OpenSpec 结构，语义由代码证据、测试与 review 负责。

## Risks / Trade-offs

- 未提交代码是基线的一部分；合并或丢弃这些修改后，必须重新核对相关规范。
- Atlas scoped search 只覆盖查询范围，本次结合直接源码和现有测试阅读，没有声称全仓库调用图完备。
- 历史审计结论尚未逐项复测，只能作为候选债务；迁移不将它们提升为当前需求。
- OS Sandbox 的保障受平台和 feature 约束；规范不声称跨平台完全等价。

## Migration Plan

读取实现并记录证据 → 生成 delta 与治理配置 → 更新开发者入口和历史索引 → 校验与归档 → 校验主规范和归档。回滚只移除本次新增文档/CI并恢复 README、ROADMAP 指引，不触碰既有代码修改。

## Open Questions

无阻止本次文档迁移的问题。完整 provider 失败取证、Workflow sidecar 恢复、Trust 与 Sandbox 后续边界继续保留在 backlog，需另行启动。
