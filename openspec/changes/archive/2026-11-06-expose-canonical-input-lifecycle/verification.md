# Verification

父核对提供方专属patch：Command/Handle/actor只委托既有Timeline投影；私有互斥consumption替代布尔，Consumed与Handled的判定和Already-consumed规则保持，Superseded仍由dismissed折叠；持久event schema不改。Shell只删除Handled二次扫描，wire形状不变。

整合后首轮编译失败E0599：actor/tests.rs冷重放对同步Timeline::input_state的Option写了两个unwrap；仅删两处多余unwrap，异步Handle的Result<Option>保留双unwrap。后续实际执行：

- `cargo +1.93.1 test --locked --offline -p chat-state --lib -- input_state_query timeline::tests --test-threads=1`：**150 passed / 0 failed / 3 ignored / 490 filtered**，exit0。父亲核原始终端日志 `call_8cd41e1b202a4d6aadebad85.log`（child `01a1af3a-625c-7091-a8ec-a13ead46ede5` terminal），实际running153，两个input_state_query均通过。三个ignored是既有测试，未改忽略状态。
- `cargo +1.93.1 test --locked --offline -p shell --lib -- subagent_terminal_commit_tests extensions::session_admin --test-threads=1`：**12 passed / 0 failed，exit0**，其中7项SessionAdmin含durable_input_status生命周期与损坏证据拒绝。完整日志 `/tmp/grow-backlog-child-end-fix-units.log`。
- 默认CLI在此提供方整合源码上build通过（task `01a1af3c-80c9-79b3-bedc-95bed611e0a1`）。

查询是纯观察，不写事件或触发执行。父同步开发者说明并将F3消费方作为独立change交接；本项通过不表示F1/F2/F3调用方已修复。仅本机验证，未声称Windows现场或所有平台已测。
