# Grow OpenSpec

`specs/` 保存当前已归档契约，`changes/` 保存提案到验证的过程，`changes/archive/` 保存已完成变更。开发者从 [docs](../docs/README.md) 了解代码，开始修改前回到这里确认契约。

## 能力索引

| Capability | 范围 |
| --- | --- |
| [session-timeline](specs/session-timeline/spec.md) | 会话事实、模型可见上下文和恢复边界。 |
| [input-admission](specs/input-admission/spec.md) | 用户输入的持久化接纳、排队与恢复。 |
| [behavior-goal](specs/behavior-goal/spec.md) | 协作协议切换与跨 turn Goal 生命周期。 |
| [context-compaction](specs/context-compaction/spec.md) | 模型上下文压缩与异步结果提交。 |
| [workflow-execution](specs/workflow-execution/spec.md) | Rhai Workflow Definition、Run 与 journal 恢复。 |
| [tool-authorization](specs/tool-authorization/spec.md) | 工具调用的权限判定与子 Agent 能力上限。 |
| [configuration-rules](specs/configuration-rules/spec.md) | 个人配置、项目配置与 Agent 规则发现。 |
| [model-sampling](specs/model-sampling/spec.md) | Provider 无关采样、流转换与请求管理。 |
| [memory-search](specs/memory-search/spec.md) | 跨会话 Markdown 记忆及可降级检索。 |
| [extension-runtime](specs/extension-runtime/spec.md) | MCP 与 Hooks 的运行时边界。 |
| [local-coordination](specs/local-coordination/spec.md) | 本机 peer 会话间的询问协议。 |
| [client-surfaces](specs/client-surfaces/spec.md) | CLI、终端交互、headless 和本地存储入口。 |
| [sandbox-boundary](specs/sandbox-boundary/spec.md) | 操作系统级隔离与平台能力边界。 |
| [development-workflow](specs/development-workflow/spec.md) | 本次迁移正式采用的项目开发契约；这是新增治理要求，不是对迁移前代码行为的描述。 |

## 开始开发

遵循 [AGENTS.md](../AGENTS.md) 和 [开发指南](../docs/development.md)。先检查现有 change，再建立本次最小变更。不要直接把未完成需求写进 `specs/`。

- [基线与覆盖范围](baseline.md)：代码证据、模块映射、历史资料和验证限制。
- [待启动事项](backlog.md)：长期计划与架构债务，不构成实现授权。
- [官方 CLI 文档](https://github.com/Fission-AI/OpenSpec/blob/main/docs/cli.md)：本仓库使用 1.11.0。

这里的规范是逐项证据支持的核心行为基线，不是所有内部函数、CLI 参数或平台行为的穷举。新增或修改尚未覆盖的行为时，先补入对应 change 的 delta。
