# Design

已有 RuntimeState 只允许 Running → SuspendRequested → … → ExecutionReleased → Running。首次 freeze 的副作用之前必须确认 root SuspendRequested；写入 ACK 未确认时不发送 freeze，后续查询已有状态对账。root 准入标志仍先关闭，避免持久确认期间继续派发。

child 的同步 freeze_admission 早于节点 SuspendRequested，因此单独前移 root 意图不足以保护等待。交互折叠保留 Suspended，ExecutionReleased 清除上一恢复周期的交互证据；随后新的 Opened 建立新的 attempt。真实 Answered/Allowed/Rejected/Cancelled/TimedOut/Failed 仍按精确 attempt 删除。最终必须匹配当前 SuspendRequested operation。实际工具结果仍由既有 unresolved-call 分类优先处理。

不增加协调状态，不把冻结 ACK 超时当成撤销。未完成节点重试、已挂起节点幂等、整树 barrier 均沿既有流程。
