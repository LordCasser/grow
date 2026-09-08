# Why
Prompt/Tool gate 的 JSON allow 当前覆盖命令非零退出码和 HTTP 非 2xx 状态，导致执行失败被记为成功，on_failure=block 无法发挥作用。显式 deny 已具有拒绝优先语义，应继续保留。

# What Changes
allow 只在命令 exit 0 或 HTTP 2xx 时有效；命令 exit 2 仍拒绝，其余失败走既有失败策略。

# Impact
仅 Prompt/Tool gate 决策与执行状态冲突处理；不修改 Stop、Observe 或成功状态下非 JSON 容忍行为。
