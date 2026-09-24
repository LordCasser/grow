## Why

当前 primary prompt 已要求主 Agent 掌握全局、保留中心证据判断，并在子任务运行时继续独立工作；subagent prompt 也已有范围与证据约束。但依赖粒度、探索与实现的就绪条件、部分成果验收、前提失效处理和并发收敛仍主要依赖模型自行补全，容易出现整批等待、重复探索或成果返回后缺少集成验证。

本 change 将这些原则落实为适配 Grow 现有工具语义的英文协作提示词。参考输入是会话「并行编程提示词 design」的无 Jev 方案，以及本任务中用户确认的英文 prompt 要求。

## What Changes

- 替换 primary audience 的协作正文：围绕最小充分输入拆分任务，按依赖选择工作，在自然检查点验收和调整计划，按整合能力控制并发。
- 替换 subagent audience 的职责正文：边界内自主执行、围绕明确未知开展探索、局部阻塞处理、验证和可整合的结果返回；保留现有 capability authority。
- 精简 `general-purpose` 的重复职责；为 `explore` 明确证据充分后的停止条件、覆盖范围和可消费的调查产物。
- 在现有 `task`、task output 工具说明中补充最小委派信息和等待选择指导；不添加参数、状态、调度 API 或消息通道。
- 记录逐处 prompt 修改分析、完整英文候选文本、装配回归和模型行为评估方案。说明 prompt 契约与运行时强制边界的区别。

本 change 最初按用户要求交付方案；2026-09-22 用户进一步授权专项实施 prompt，并要求与正在修改代码的 Agent 协调。实施范围为下述文案、相关测试和开发者说明，具体证据见 `verification.md`。保留 delta specs，不设置 `skip_specs: true`。2026-09-24 对原定大规模模型实验重新评估价值后，完成三组受控配对 pilot，并明确记录其余未执行场景；本次归档依据是指引文本、装配和工具语义的确定性证据，不宣称普遍模型行为或性能改善。

非目标：Jev 接入、自动模型/effort 路由、新 DAG 调度器、任务数据库、权限调整、通信协议或 TUI 改造、取消与恢复生命周期修复、文件原子冲突保护。

## Capabilities

### New Capabilities

无。复用已有本地协作能力和提示词装配，不新增运行时实体。

### Modified Capabilities

- `local-coordination`: 增加模型可见的依赖驱动协作指引契约，规定主从责任、委派输入、就绪与验收区分、写入协调、真实通信语义和结果返回；原有 Sideband、消息回执、授权和生命周期要求保持原义。

## Impact

- 生产修改：`crates/codegen/agent/prompts/audience/{primary,subagent}.md`、`prompts/agents/{general-purpose,explore}.md`，以及 `crates/common/tool-types/src/task.rs` 中现有工具说明正文。
- 验证：`crates/codegen/agent/src/prompt/{template,context}.rs`、`agent/src/builder.rs` 和 `tool-types/src/task.rs` 的相关回归；模型行为场景与配对对照单独记录实际完成状态。
- 开发者说明：更新 `crates/codegen/agent/PROMPT_ARCHITECTURE.md` 并链接规范；经协调，无需修改 `docs/architecture/local-coordination.md`，不在 docs 复制整份 prompt 或维护第二套任务清单。
- 不改 PromptContext 装配顺序、工具 schema、默认后台启动、wait-all、Sideband、权限 Gate、Goal/Workflow 状态所有者与持久化格式。
- 与 `clarify-agent-communication-context-and-rendering` 共享通信语义证据，但不依赖其 UI 实现；不合并 `fix-cancelled-subagent-resume-lifecycle` 的修复；Jev 设计仍属于独立事项。

阅读顺序：[现状与逐处修改](prompt-analysis.md) → [设计](design.md) → [英文候选文本](prompt-drafts.md) → [delta spec](specs/local-coordination/spec.md) → [验证计划](validation-plan.md) → [任务](tasks.md)。当前完成情况见 [verification.md](verification.md)。
