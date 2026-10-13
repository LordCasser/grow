## ADDED Requirements

### Requirement: Recovery dependencies constrain automatic resource reclamation

自动 session、artifact、worktree 及 snapshot 回收 SHALL 保留未闭合子任务链、未确认输入和未完成交付所需的可验证资源。清理资格 SHALL 由持久身份、引用及实际 owner 规则决定，不仅由 TTL、最近 activity、runtime handle 或 completed cache 决定。无需通知的 child 及已持久消费/按规则 dismissed 的回执 SHALL 按各自事实判定，不人为制造永久交付依赖。

#### Scenario: Inactive child has an unresolved parent spawn
- **WHEN** 旧 writer 已退出且 TTL 已到，但原 child/parent 结果链尚未闭合
- **THEN** 自动清理保留恢复所需 session、结果和工作资源，并报告保留原因。

#### Scenario: Parent terminal exists but delivery is pending
- **WHEN** parent Ended 已提交，但对应结果尚需回执或消费
- **THEN** 清理保留尚需读取的结果及关联证据，不能因任务已显示完成就删除。

#### Scenario: Worktree snapshot failed on a full disk
- **WHEN** 含修改的工作树未取得可验证持久 snapshot 及必要引用
- **THEN** 原工作树保持可恢复，正常收尾和自动 GC 都不能把快照尝试当作保全成功。

#### Scenario: Internal child needs no completion notification
- **WHEN** 原 spawn 明确不产生模型完成回执且其其他生命周期义务已确认
- **THEN** 不因不存在本就无需创建的回执而保留无效交付依赖，仍需满足其他清理条件。

证据与实施入口：`crates/codegen/shell/src/session/storage/jsonl/mod.rs::cleanup_stale_sessions_sync`、`crates/codegen/shell/src/agent/subagent/handle_request.rs`、`crates/codegen/workspace/src/worktree/mod.rs`、`crates/codegen/fast-worktree/src/api.rs`。

### Requirement: Destructive maintenance revalidates recovery references at deletion

自动维护 SHALL 在实际删除的所有权边界内重新核对同一资源实体、引用和可回收资格，并与相关发布/写者操作串行化。扫描缓存、临时路径相同或无 live handle 不构成删除许可。无法取得所需所有权、校验失败、扫描超预算或缺少上层恢复证据时 SHALL 保留该候选；仍在运行的读取或删除不得因 caller 超时而被重复派发。

#### Scenario: New reference appears after candidate scanning
- **WHEN** 资源曾被扫描为候选，但在取得删除所有权前产生了应保留的引用
- **THEN** 删除边界的重新核对发现引用并保留原资源，不依据旧扫描结果删除。

#### Scenario: Standalone worktree GC lacks session authority
- **WHEN** 独立 GC 遇到任务关联 worktree 或其 snapshot，但不能验证相应恢复义务
- **THEN** 普通自动清理跳过该资源，不以年龄或进程消失替代资格证明。

#### Scenario: Recovery dependency scan cannot complete
- **WHEN** 所需记录不可读、损坏或扫描达到预算
- **THEN** 报告本轮资格未确认并保留候选，未扫描的引用不被解释为不存在。

#### Scenario: Fresh pristine worktree was never admitted
- **WHEN** 创建失败有明确证据表明新 pristine worktree 尚未发布 spawn、未交给执行器且没有引用
- **THEN** 允许既有回滚清理；ACK 不明或包含用户修改的资源不能使用此例外。

证据与实施入口：`crates/codegen/shell/src/session/storage/jsonl/mod.rs::delete_if_still_stale`、`crates/codegen/shell/src/session/worktree.rs::cleanup_worktree_on_failure`、`crates/codegen/workspace/src/worktree/mod.rs::run_auto_gc_best_effort`、`crates/codegen/shell/src/session/input_inbox.rs`。

### Requirement: Confirmed storage failure bounds new execution admission

已确认使持久写入不可用的故障 SHALL 关闭受影响 owner，或当前运行管理实例内已确认共享故障存储域的新执行准入，包含 prompt promotion、child spawn 和后续 provider/tool 派发。故障范围 SHALL 根据来源与存储身份确认，不能仅凭 errno 影响无关域或声称控制其他独立进程。诊断、取消及合法收尾恢复仍可执行。准入恢复 SHALL 确认有效 writer 的持久屏障，并继续受既有 Goal/权限/预算约束；剩余磁盘字节变化本身不构成恢复成功。运行中的外部副作用不得被声称已经撤回。

#### Scenario: Shared session storage reports no space
- **WHEN** 写入确认共享 Grow 存储域空间或配额耗尽
- **THEN** 当前管理实例内该域不继续准入依赖持久化的新工作，已有任务保持可观察和可请求取消，收尾责任仍被保留。

#### Scenario: Failure belongs to a single owner
- **WHEN** 一个 session 的权限或完整性故障没有证据影响其他存储 owner
- **THEN** 关闭该 owner 的准入，不无依据关闭无关域。

#### Scenario: Space recovers but old writer remains invalid
- **WHEN** 磁盘空间恢复而旧 writer 已失效或尚未退出
- **THEN** 不重开执行；合法 writer 恢复并确认持久屏障后才重新评估准入。

证据与实施入口：`crates/codegen/chat-state/src/persistence.rs::persistence_error_is_permanent`、`crates/codegen/shell/src/session/actor/run_loop.rs`、`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs`；此处新增故障准入契约，不替代既有 Goal 准入权威。
