## Context

`ValidatedTimeline` 同时持有原始 events 和已折叠 Timeline；light load 仅返回 events，丢弃折叠结果。bootstrap 再从 events 构造 Timeline，并为稍后的 Workflow 恢复复制整份状态。

## Decisions

1. 由 light load 携带现有 Timeline 类型，进入 `TimelineBootstrap::Existing` 后直接转移给 ChatState。保持 full load 的既有事件输出，不另设缓存或索引服务。
2. blob/sideband 验证仍在 pinned storage entity 上执行，input payload 与 stable System 校验仍在 actor 发布前执行。新的用量恢复校验仍可拒绝 actor；不能为了少一次验证而接受无效事实。
3. Workflow 恢复只需受限 run 的生命周期。避免持有整份 resumed Timeline 时，必须保持其修复持久化晚于 ChatState 校验，不把有副作用的 from_restored 提前执行。
4. 首先测量经过修复的 fixture，记录其源码、样本尺寸、运行参数和限制；普通 synthetic 样本控制在 256 MiB 内，复用同一 target 串行编译。不同体量的 Timeline 与 updates 分别对照，不以空 Timeline 证明恢复优化。
5. 已有 replay cutoff、cursor、gateway 完成屏障、writer lease 和 usage resume boundary 保持。第二次 updates 读取与更广 resident 快路径仅在测量支持且能保持这些约束时另行处理。

## Validation

先记录 cold load 的第一条/最后一条通知与完整响应；定向验证 storage light/full 投影一致、既有损坏拒绝、子会话 bootstrap、Workflow 恢复和协议回放。Pager 输入延迟由单独交互变更测量，shell ACP 吞吐不冒充终端响应时间。
