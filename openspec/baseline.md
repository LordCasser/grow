# 代码基线

迁移日期：2026-09-07。分支：`codex/openspec-sdd`。起点 HEAD：`1e1fda6d2f7b3990d2c0a34d0c0015b9b1edbb5c`。
基线来自实际工作树，包含迁移开始前的未提交代码，不能等同于该 commit 或已发布版本。规范中的源码路径和符号用于追溯；[本次归档的 evidence.json](changes/archive/2026-09-07-establish-openspec-baseline/evidence.json) 保存读取源的 SHA-256 和当时 Git 状态。

## 覆盖范围

本次建立 14 个 capability、41 项要求。`development-workflow` 是本次新采用的治理契约，其余是从实现与现有测试提取的核心行为。源码测试名作为可复查证据，不代表本次已经运行这些测试。

Atlas 仅执行工作区打开与 Workflow 局部符号查询；其余证据直接读取源码、manifest 与测试。没有做全仓库完整调用图提取，也没有把旧文档当成实现证明。

## 模块映射

Cargo workspace 的 57 个成员均在下表登记。分组只帮助阅读，不增加运行时实体，也不声称每个辅助 crate 都有独立产品契约。

| 分组 | Cargo 成员 |
| --- | --- |
| 入口与展示 | `cli`, `pager`, `pager-minimal`, `pager-render`, `pager-pty-harness`, `ratatui-inline`, `ratatui-textarea`, `markdown`, `mermaid`, `tty-utils`, `client-support` |
| 会话与模型 | `shell`, `shell-base`, `agent`, `chat-state`, `sampler`, `sampling-types`, `token-estimation`, `compaction`, `prompt-queue` |
| 工具与执行 | `tools`, `tool-protocol`, `tool-runtime`, `tool-types`, `ptyctl`, `ptyctl-cli`, `hunk-tracker` |
| 工作区与隔离 | `workspace`, `workspace-types`, `sandbox`, `fast-worktree`, `fsnotify`, `grow-gix-status` |
| 扩展与配置 | `config`, `config-types`, `paths`, `auth`, `hooks`, `mcp`, `plugin-marketplace`, `extension-types`, `acp-transport` |
| 记忆与工作流 | `memory`, `workflow`, `sqlite-journal`, `codebase-graph` |
| 运行支持 | `diagnostics`, `crash-handler`, `grow-http`, `update`, `version`, `announcements`, `tracing-macros`, `test-support`, `test-utils` |
| 第三方布局 | `dagre_rust`, `mermaid-to-svg` |

实际依赖以根 Cargo.toml 与各 crate Cargo.toml 为准。vendored patch 的原因仍保留在 docs/architecture/dependency-workarounds.md 与 third_party 的说明中。

## 已知限制

- 规范描述支持路径的实际行为，不能推导出未验证的所有平台、所有 Provider 或所有失败窗口都已覆盖。
- 历史文章可能含旧版本描述；Goal 和 Memory 已知漂移登记在 backlog。遇到其他偏差，通过 change 核对并修正。
- 用户已有未提交修改未被本次重写、提交或回滚。后续集成这些代码时，重新核对受影响规范和测试。
- 文档 CI 保证结构及归档任务完成标记，不能证明 Rust 行为或文字语义。

## 历史资料

长期计划已转入 [backlog](backlog.md)。下表冻结旧资料的过程管理用途，保留 docs 阅读副本与原链接；从迁移后开始，新增或继续的审计、设计、任务和验收只在 OpenSpec change 管理。

| 资料 | 定位 |
| --- | --- |
| [agent-core-timeline.md](../docs/architecture/agent-core-timeline.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |
| [behavior-state-overview.md](../docs/architecture/behavior-state-overview.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |
| [compaction-pre-prune.md](../docs/architecture/compaction-pre-prune.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |
| `docs/architecture/continuous-feature-audit.md`（本地未跟踪文件） | 迁移前用户未提交审计，原样保留；结论未在本迁移逐项复验。 |
| [dependency-workarounds.md](../docs/architecture/dependency-workarounds.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |
| [goal-continuation.md](../docs/architecture/goal-continuation.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |
| [input-routing.md](../docs/architecture/input-routing.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |
| [local-coordination.md](../docs/architecture/local-coordination.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |
| [pager-hyperlinks.md](../docs/architecture/pager-hyperlinks.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |
| [pager-motion.md](../docs/architecture/pager-motion.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |
| [project-review-2026-09-05.md](../docs/architecture/project-review-2026-09-05.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |
| `docs/architecture/project-review-2026-09-07.md`（本地未跟踪文件） | 迁移前用户未提交审计，原样保留；结论未在本迁移逐项复验。 |
| [session-robustness-repair.md](../docs/architecture/session-robustness-repair.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |
| [slash-command-feedback.md](../docs/architecture/slash-command-feedback.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |
| `docs/architecture/trajectory-review-2026-09-07.md`（本地未跟踪文件） | 迁移前用户未提交审计，原样保留；结论未在本迁移逐项复验。 |
| [truncation-recovery.md](../docs/architecture/truncation-recovery.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |
| [upstream-borrow-review.md](../docs/architecture/upstream-borrow-review.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |
| [v1.0.0-regression-analysis.md](../docs/architecture/v1.0.0-regression-analysis.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |
| [workflow-workspace.md](../docs/architecture/workflow-workspace.md) | 既有说明或修复历史，保留供开发者查阅；不作为进行中任务表。 |

产品 README、crate 用户指南、Workflow Rhai 和平台开发说明继续服务开发者，不视为第二套规范。
