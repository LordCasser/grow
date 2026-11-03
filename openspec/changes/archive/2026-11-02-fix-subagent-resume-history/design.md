## Context

`SubagentSpawned` 在 parent loading_replay 期间仅创建 child view；`ensure_subagent_child_replayed` 到首次打开才恢复 transcript。其内容检查只允许 prompt/notice/received row，任何较早抵达的实时输出或完成 footer 都会永久阻止恢复。挂起树恢复沿用 child 原 identity，本次不涉及 Task `resume_from` 派生新 child 的路径。

## Decision

复用既有历史重放入口，在 child ACP 实时更新及会改变 transcript 的 Grow 实时更新/child 终态应用前执行 deferred replay。重放标记保证只读一次；parent loading_replay 不阻止实时衔接，也不把普通 replay/control snapshot 当成需要加载全部 child 历史的信号。执行权限、持久化、模型可见上下文保持不变。

root loading 期间尚未恢复的 child 完成时沿原路径延迟 footer；若首条实时正文已经恢复其历史，则该 live terminal 正常收束一次，避免 root 的加载状态使 footer 永远丢失。

## Verification

历史与实时通知可能重叠：沿现有 child ACP 高水位去重带 eventId 的已恢复记录；writer 恢复时既有 `HistorySnapshot::reseed_event_counter` 将新 transport counter 提升到 durable source ID 上方，不把 Timeline domain seq 当 transport counter。canonical response projector 会移除 eventId，因此使用其原 request/attempt 身份调用既有 Accepted 生命周期，阻止已恢复回答的迟到预览追加。Grow 与 ACP 不共享水位。

使用真实 journal fixture 经生产通知路由恢复 prompt、旧工具记录，再输入实时 Thinking/正文、工具或完成事件。核对顺序、一次性重放和重新打开的幂等性，同时保留原延迟加载回归。
