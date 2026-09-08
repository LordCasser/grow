## Why

现有 14 份核心规范不足以代表整个仓库。用户要求逐个遍历 crate，发掘所有功能特性并写入 OpenSpec，不能用 manifest 分组或少数核心路径代替功能覆盖。

## What Changes

- 逐项核查全部 61 个 Cargo package：57 个 workspace 成员，以及 fuzz、vendored 等 4 个非成员包。
- 为每个包记录 public/module 入口、feature 开关、平台分支、主要行为与失败边界；大型 shell、pager、tools 按模块和命令族继续拆解。
- 对每个已发现功能建立到主规范或本 change delta 的映射；未核查包、未映射功能保持 pending，不以格式通过当完成。
- 增补能力规范、开发者索引与可复查证据；不修改 Rust 行为，不实施发现的架构债务。

## Capabilities

### New Capabilities

- `acp-transport`：按逐包证据补充已实现功能。
- `application-maintenance`：按逐包证据补充已实现功能。
- `chat-state`：按逐包证据补充已实现功能。
- `cli-entrypoints`：按逐包证据补充已实现功能。
- `codebase-navigation`：按逐包证据补充已实现功能。
- `crash-reporting`：按逐包证据补充已实现功能。
- `dagre-graph-layout`：按逐包证据补充已实现功能。
- `developer-support`：按逐包证据补充已实现功能。
- `diagram-rendering`：按逐包证据补充已实现功能。
- `filesystem-events`：按逐包证据补充已实现功能。
- `hook-execution`：按逐包证据补充已实现功能。
- `http-credentials`：按逐包证据补充已实现功能。
- `hunk-attribution`：按逐包证据补充已实现功能。
- `inline-terminal-runtime`：按逐包证据补充已实现功能。
- `local-diagnostics`：按逐包证据补充已实现功能。
- `mcp-integration`：按逐包证据补充已实现功能。
- `mermaid-svg-rendering`：按逐包证据补充已实现功能。
- `minimal-terminal`：按逐包证据补充已实现功能。
- `process-lifecycle`：按逐包证据补充已实现功能。
- `pty-control`：按逐包证据补充已实现功能。
- `release-update`：按逐包证据补充已实现功能。
- `sqlite-storage`：按逐包证据补充已实现功能。
- `terminal-markdown`：按逐包证据补充已实现功能。
- `test-harness-runtime`：按逐包证据补充已实现功能。
- `textarea-editing-runtime`：按逐包证据补充已实现功能。
- `tool-runtime`：按逐包证据补充已实现功能。
- `vector-storage`：按逐包证据补充已实现功能。
- `workspace-git-status`：按逐包证据补充已实现功能。
- `workspace-paths`：按逐包证据补充已实现功能。
- `workspace-rpc`：按逐包证据补充已实现功能。
- `worktree-lifecycle`：按逐包证据补充已实现功能。

### Modified Capabilities

- `behavior-goal`：补充本次遍历发现的现有行为要求。
- `client-surfaces`：补充本次遍历发现的现有行为要求。
- `configuration-rules`：补充本次遍历发现的现有行为要求。
- `context-compaction`：补充本次遍历发现的现有行为要求。
- `extension-runtime`：补充本次遍历发现的现有行为要求。
- `input-admission`：补充本次遍历发现的现有行为要求。
- `memory-search`：补充本次遍历发现的现有行为要求。
- `model-sampling`：补充本次遍历发现的现有行为要求。
- `sandbox-boundary`：补充本次遍历发现的现有行为要求。
- `session-timeline`：补充本次遍历发现的现有行为要求。
- `tool-authorization`：补充本次遍历发现的现有行为要求。
- `workflow-execution`：补充本次遍历发现的现有行为要求。

能力清单随尚未完成的 crate 遍历继续扩展。

## Impact

仅文档和规范。代码基线是独立 codex/openspec-sdd 工作树；不读取 main 未提交修改作为此轮事实，不引入功能审计 change。
