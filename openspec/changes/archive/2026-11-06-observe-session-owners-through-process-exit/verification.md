# Verification

父亲核最终主工作树diff：Activity生产注册强制传入既有SessionThread，存活与flush完成条件观察actor及persistence共同退役；命令通道关闭仅封闭准入。map/reaper仍保留既有join责任，按channel区分同ID不同incarnation，50ms重取快照与单次10秒grace均保留。

独立只读review `01a1af82-c1f4-74b3-aa84-21a6461cc18f` 检查实际activity.rs、agent_ops.rs注册/失败路径及spawn.rs sharedArc/Joining/Joined/is_finished/Drop，未发现可行动缺陷。首位reviewer因网络失败0tool没有审查证据，已明确解除并由上述review替代。

父main运行 `/tmp/grow-backlog-shell-integrated-final.log`：Rust1.93.1、locked/offline、单线程，统一Shell96项全部通过，包含14项Activity及3项真实owner测试（closedchannel+liveactor、actor退出+persistence仍活、完整退休后entry清理）。此前coder隔离环境的14Activity、1autoupdate和138leader通过仅作补充，不替代main证据。

`cargo +1.93.1 build --locked --offline -p cli --bin grow`实际exit0，日志 `/tmp/grow-backlog-cli-integrated-build.log`。新binary运行普通非leaderheadless的`test_session_end_hook_e2e` **1 passed / exit0**，外部Hook marker、stdin envelope及reason=shutdown原断言执行；同编排child正常/取消 **2 passed**。日志 `/tmp/grow-backlog-terminal-e2e-integrated.log`。

原ordinary复现的prompt成功但marker缺失、flush后Timeline ACKlost日志保留于child顺序change的诊断历史；旧CLI一次green不能否定调度竞态，本修复由真实actor/persistence barrier固定反例。未改正常用户配置/session，全部产品链使用隔离mock/tempdir。Windows实机未运行，不声称验证了Windows现场根因或同步I/O硬终止。
