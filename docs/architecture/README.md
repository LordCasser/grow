# 架构阅读顺序

这里保留给开发者看的机制解释。行为要求与验收场景以 [OpenSpec](../../openspec/README.md) 为准；既有文章中的旧版本号和修复过程不能覆盖当前规范。

| 阅读材料 | 关注点 | 对应规范 |
| --- | --- | --- |
| [Agent Timeline](agent-core-timeline.md) | 事实、Surface 与请求投影 | [session-timeline](../../openspec/specs/session-timeline/spec.md) |
| [输入路由](input-routing.md) | 输入接纳、FIFO 与前台所有权 | [input-admission](../../openspec/specs/input-admission/spec.md) |
| [Behavior](behavior-state-overview.md)、[Goal](goal-continuation.md) | 协作协议与跨 turn 续跑 | [behavior-goal](../../openspec/specs/behavior-goal/spec.md) |
| [压缩](compaction-pre-prune.md) | 冻结输入与边界提交 | [context-compaction](../../openspec/specs/context-compaction/spec.md) |
| [Workflow](workflow-workspace.md) | Definition、Run 与持久化 | [workflow-execution](../../openspec/specs/workflow-execution/spec.md) |
| [本机协调](local-coordination.md) | peer、询问与取消 | [local-coordination](../../openspec/specs/local-coordination/spec.md) |
| [Pager Motion](pager-motion.md)、[链接](pager-hyperlinks.md) | 展示层机制，具体细节尚未穷举为规范 | [client-surfaces](../../openspec/specs/client-surfaces/spec.md) |
| [依赖 workaround](dependency-workarounds.md) | Cargo patch 保留原因 | [模块与覆盖范围](../../openspec/baseline.md) |

大致调用顺序是 `cli → pager/ACP → shell SessionActor → chat-state / sampler / tools`。`chat-state` 持有可重放事实，`shell` 决定何时接纳、运行和提交，`pager` 展示投影；具体依赖由各 crate 的 Cargo.toml 和实现确认。

历史审计与修复文章仍可通过原路径查阅，但新过程记录只写入 OpenSpec change。详情见 [历史资料登记](../../openspec/baseline.md#历史资料)。
