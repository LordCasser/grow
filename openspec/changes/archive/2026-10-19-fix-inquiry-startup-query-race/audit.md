# 根因与测试覆盖复盘

## 首个失败及清理日志

发布 run [39191289841](https://github.com/LordCasser/grow/actions/runs/39191289841) 的 `scripts/test_local_coordination.py:307-313` 在并发启动 queued inquiry 后轮询 `get`，只接受尚未接纳时的 `not_found` 或正确的 queued 状态，实际收到 `source_unavailable`。

脚本 `finally` 会先释放模型阻塞、关闭 clients，再输出它们的 stderr 和原异常堆栈。stderr 中的 `session persistence actor unavailable` / `writer epoch poisoned` 可能在清理期间产生；没有在断言前确认 writer 故障的证据，不能据此推断源进程崩溃、存储损坏或 ARM64 特有问题。最初把这些日志直接当成根因的解释已撤回；Windows x86_64 也复现相同断言。

## 两个观察窗口

1. `SessionCoordinationBackend::ask_with_id` 先 await source actor 的 `OutgoingStarted` durable ACK，随后才 spawn 调用 `CoordinationHandle::ask_session` 并登记 outgoing record。
2. `get_inquiry` 先在 outgoing map 找记录，未命中时释放锁，再读取 canonical Timeline。即使启动登记已改为原子，查询仍可能先错过 map，随后读到另一个启动的 start。

新 inquiry 在这些窗口中与“新 runtime 冷恢复只有历史 start”具有相同持久化外观；当前查询将其一律判为 `SourceUnavailable`。触发概率取决于调度和 I/O 时序，多平台失败不构成硬件归因。

修复先登记现有 outgoing 运行记录，再于锁外执行 actor durable admission；query 的历史回退与登记使用同一锁，运行事实不会在 ACK 前派发，历史 start 的冷恢复语义仍保留。接纳期间取消保持可用。

## 为什么已有测试没有提前发现

- actor 的 `source_startup_admission_survives_rebuild_without_duplicate_append_or_notice` 测持久化接纳去重、重建和冲突拒绝，不包含 runtime outgoing 登记或并发 query。
- runtime 的并发重试测试直接调用 `ask_session`，没有生产 backend 在外部先落盘再登记的 await 边界。冷恢复测试手工写入 start 再创建新 runtime，正确地预期 `SourceUnavailable`，没有区分活启动。
- 此前最终验证运行 Shell 包级测试和会话树/Plan/Skill/按键 PTY；记录中没有运行独立进程 `scripts/test_local_coordination.py`。因此大量单测通过不等于这个接合场景已验证。旧验证记录的限定范围保留，不改写成已覆盖。
- `core-regression.yml` 和 `local-coordination.yml` 此前只由 PR 或 workflow_dispatch 触发，直接推送 main 不会启动它们。此次发布前静态 workflow 检查只证明 tag、版本及资产契约，不证明进程协调正确。
- 现有真实进程脚本的断言实际有效，发布 smoke 正是由它阻止公开；不应将接受 `SourceUnavailable` 或重跑到通过当作修复。

## 本次补强

- 使用 oneshot 屏障确认真实 start 已落盘但 admission ACK 尚未放行，主动 query；验证不会把活启动归为 orphan，ACK 前目标没有请求，同 ID 重试只有一次 admission。
- 补 admission 失败无派发、取消后无目标执行，并保留 cold-source、payload conflict、独立 source identity 等既有回归。
- 运行现有 CLI 独立进程脚本，核对新二进制来源；记录真实命令、失败及通过，不以历史二进制代替本次构建。
- 为相关 main push 添加原有协调 CI，同样运行库测试和真实进程脚本。发布矩阵中的实际产物 smoke 门禁保留。

完整命令和结果见本 change 的 `verification.md`。修复完成、验证与归档之前不勾选 tasks。
