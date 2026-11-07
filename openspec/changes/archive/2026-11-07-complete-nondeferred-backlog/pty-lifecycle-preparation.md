# PB-03 生命周期实施准备

本记录只保存父在问答绘制诊断期间核实的接口事实，不代表PTY行为已修复或已运行CLI反例。原审计见`../archive/2026-11-05-audit-grow-architecture-across-versions/reviews/product-build.md`的PB-03；后续先独立change/delta再交实现。

## 当前可复核边界

- `ptyctl/src/session.rs::start`创建shutdown通道却丢弃receiver，timeout/linger未被消费；`stop`take sender后忽略send结果。reader EOF先写alive=false，child waiter看到它就退出，未保证取得真实exit_code。输出EOF、子进程退役和服务终止目前并非同一事实。
- `ptyctl/src/server.rs::handle_stop`持session lock调用stop且无条件HTTP成功；WS仅靠output broadcast关闭发送terminal，但session保留sender。现wait_handle已示范克隆必要句柄后释放外层锁再await，不应为stop新增长期持锁等待。
- `ptyctl-cli/src/commands/run.rs`先启动child再bind/注册，serve无graceful shutdown；启动后bind或注册错误必须由该次实际持有的child owner清理。名称--force明确只替换注册，无权按旧registry PID停别的服务；清注册只允许仍指本次port的记录。
- CLI help定义timeout为“Shutdown after N seconds”、linger为“Keep server running after process exits”、stop为“Stop a running session”。自然退出与显式stop/timeout的服务政策需在delta中明确区分，不能让linger撤销用户明确关闭。
- 锁定portable-pty 0.9.0：Child提供Result型try_wait/wait；PtyChild当前is_alive用ok().flatten丢弃观察错误。其Unix spawn执行setsid，但默认std::process::Child的kill以及clone_killer只针对该child，并不提供任意后代树回收保证。后续只能从真实spawn owner取得控制权，不从观测PID/名称扩权。不能仅凭setsid就宣称调用现kill会清理全组。

## 后续单项实现前需要冻结

Session是child控制与结算owner；CLI/server消费它的完成信号并承担HTTP服务和名称注册生命周期。优先复用当前channels/watch和实际child handle，不建通用进程管理框架。stop成功必须对应真实结算或明确失败；输出EOF不能替代child exit，最终输出/WS terminal与exit_code须一致。接口应允许server取得不持外层session lock的shutdown future/handle，具体最小表示待实现者核当前调用方后选择。

必要真实CLI反例包括显式stop、timeout、自然退出与linger保留，以及spawn后bind/注册失败。它们检验同一缺失的lifecycle接线，不把Pager harness的独立child/tree owner带入此修复。先确认直接child及其I/O责任的边界；如要求清理更广后代，必须单独证明合法spawn-time组/句柄与退役语义，不能把PB-04执行器的树承诺混进来。Windows未实跑的限制照实保留。

## 2026-11-07 补充的只读证据

父进一步核了锁定portable-pty原语及Shell/Pager既有结算代码，只读委派`01a1b319-c3d9-7be1-ace9-12efd087907e`补充consumer与测试入口，已作为静态资料接受。未改PTY源码、未跑Cargo/CLI/kill。仓内PtySession::start/build_router生产consumer是ptyctl-cli run；harness仅复用terminal/keys/styled，自有child/tree。两crate publish=false不等于仓外CLI/HTTP/WS无人使用。

portable-pty 0.9.0的Unix clone_killer只发一次SIGHUP，不升级或等待；direct child kill先HUP并try_wait，短轮询后升级std::Child::kill。single owner必须协调kill/poll/wait与状态，不将clone句柄等同可靠停止，也不让两个结算者竞争同一child。父亲读lib.rs270..370确认原Child是std::process::Child；不要由两种原语分别不足推断无法用同一个owner实施。PtyChild现is_alive吞try_wait错误须在正式接口中处理。

纠正只读报告的一处过强表述：UnixMasterWriter自定义Drop除发VEOF外，其字段仍会自动drop自己的fd；问题是其它reader/writer/master dup仍可能保持PTY，不是该Drop永远不关fd。Shell pty_session.rs::reap的既有说明支持“不能靠丢某一handle保证child结算”。wait.rs的Arc<PtySession>/stop(&self) TODO仅作为设计线索，不构成额外项目指令；正式change决定最小接口，不机械扩公共面清理。

父后续按现帮助冻结的最小产品方向：自然child退出时非linger关服务、linger保status/注册/最后屏幕；显式stop和run timeout终止整session，linger不能否决。controller正常关闭保自身成功exit，child退出码通过原status/WS交付，本项不把CLI改成shell wrapper；signal数值沿portablepty现表示，不先扩signal schema。force仍只替换注册，清理只删仍指自己port的一代。

Server/CLI graceful不能只挂child-end future：实际axum graceful等待连接，WS发送在慢client上也可等待；WS须由terminal驱动并有界结束，HTTP stop的成功要对应实际结算且能完成响应，不能被自己的shutdown锁死。具体grace常量及future/handle仍待独立change冻结，此准备不是精确接口实现合同。

真实CLI测试可用CARGO_BIN_EXE_ptyctl，隔离PTYCTL_SESSION_DIR与HOME/cwd/env；port0+quiet是机器可读握手。已占端口制造bind失败、registry根被普通file占位制造register失败，不需权限假设或外部服务。取本次实际spawn的child pid作观察与owned fixture回收，不从任意registry PID扩权。`/query/status`的size字段是server_alive探针，linger下serveralive和childalive分离必须保持。opened WS closed/exit_code及服务退出需要真实protocol证据；若需dev-only客户端，沿现锁定workspace依赖评估，优先于假socket框架。Session独立公共stop/terminal可先在crate内验收，Server/CLI再打通产品里程碑；两者均未实施。
