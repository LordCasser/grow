## Why

现有架构解释、修复记录和长期计划分散在 docs 与 ROADMAP，缺少明确的当前契约和变更过程入口。本次从代码建立 OpenSpec 基线，并使后续开发遵循 SDD。

## What Changes

- 以实际工作树为证据提取能力规范和场景。
- 增加项目 AGENTS.md、OpenSpec 配置、开发者入口与规范 CI。
- 长期计划转入 OpenSpec backlog；已有开发者文档保留，过程历史冻结并登记。
- 非目标：修改 Rust 行为、清理用户未提交代码、实施审计债务、全面枚举每个内部函数。

## Capabilities

### New Capabilities
- `session-timeline`: 会话事实、模型可见上下文和恢复边界。
- `input-admission`: 用户输入的持久化接纳、排队与恢复。
- `behavior-goal`: 协作协议切换与跨 turn Goal 生命周期。
- `context-compaction`: 模型上下文压缩与异步结果提交。
- `workflow-execution`: Rhai Workflow Definition、Run 与 journal 恢复。
- `tool-authorization`: 工具调用的权限判定与子 Agent 能力上限。
- `configuration-rules`: 个人配置、项目配置与 Agent 规则发现。
- `model-sampling`: Provider 无关采样、流转换与请求管理。
- `memory-search`: 跨会话 Markdown 记忆及可降级检索。
- `extension-runtime`: MCP 与 Hooks 的运行时边界。
- `local-coordination`: 本机 peer 会话间的询问协议。
- `client-surfaces`: CLI、终端交互、headless 和本地存储入口。
- `sandbox-boundary`: 操作系统级隔离与平台能力边界。
- `development-workflow`: 本次迁移正式采用的项目开发契约；这是新增治理要求，不是对迁移前代码行为的描述。

### Modified Capabilities

无既有 OpenSpec capability。

## Impact

新增文档与 CI，不改运行时依赖和 Rust 源码。OpenSpec CLI 固定为 1.11.0，需 Node.js 环境。
