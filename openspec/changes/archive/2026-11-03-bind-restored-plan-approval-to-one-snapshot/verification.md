# 验证

2026-11-03，隔离临时GROW_HOME、RUST_MIN_STACK=16777216，Cargo --locked。

- 旧代码red：`restored_plan_artifact_read_cannot_normalize_a_replacement_plan` 用真实artifact异步读完成barrier暂停A，持久安装合法counter递增B2且B正文缺失。旧代码把B清为Normal；日志 `/tmp/grow-backlog-plan-race-before.txt`，0 passed/1 failed，0.15秒。
- stale branch red：`stale_plan_approval_normalization_waits_for_exact_snapshot_transaction` 持step gate/poll恢复，旧支路在拿到gate前清内存bit，失败；日志 `/tmp/grow-backlog-plan-stale-bit-before.txt`，0 passed/1 failed，0.13秒。
- 最终 `cargo test --locked -p shell --lib plan_ -- --test-threads=4`：110 passed、0 failed，2.03秒。涵盖两项race变green、expected正文/身份竞争、malformed显式二次恢复、原PlanControl/retained/Goal/Workflow、artifact缺失/损坏与退出投影、Control ACK故障、挂起等待等。
- `cargo test --locked -p shell --lib reverse_request_session_id_tests -- --test-threads=4`：11 passed，0.10秒。保留durable Opened/Resolved、transport/malformed失败、新attempt及正常用户问题/审批边界。

新增ACK故障fixture复用既有RejectControlPersistence，成功时完整snapshot只变pending bit，Drafting正文/hash/revision保留；写入被拒绝时内存与完整已提交日志不变。最初测试尝试对无PartialEq的TimelineEvent整Vec比较产生编译错误，改成序列化值精确比较后执行通过，未放宽断言。

malformed测试使用实际actor恢复owner方法、同gateway连接；首个缺decisionId响应关闭Failed attempt，原Control/artifact未变且无Turn/handoff；仅显式第二次调用（RestorePlanApproval调用同一方法）才创建新attempt，合法Deferred只提交一项决定，不自动循环。该fixture不是完整外部client的session/load RPC自动化，不声称Bundled Pager存在malformed正常操作入口。

身份竞争fixture用state mutex与manual future poll固定正文传入后的await窗口，合法持久B替换后不发RPC、不建waiter、不改B；wrong-body hash也在开waiter前拒绝。

独立review最终通过；git diff --check通过。没有重复上一增量全Shell suite；本增量只触及Plan恢复/请求及相关测试，执行上述相关矩阵。

归档前strict18/18；归档后全量strict17/17、archive tasks787/787通过。
