## Build and test artifacts

本轮使用仓库指定 Rust/Cargo 1.93.1，命令级 PATH 为 `~/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/opt/homebrew/sbin`。默认环境优先解析 Homebrew 1.99.1，不能代表本轮工具链。native 系统工具优先，没有修改全局 PATH 或禁用 allocator/sandbox。

- `CARGO_BUILD_JOBS=4 cargo build --locked -p cli --bin grow`：通过，默认 features，5m28s。产物 `target/debug/grow`，419,289,816 bytes，`grow 2.5.5 (bbd2c4dfa) [stable]`；SHA-256 `8aeabdcf2bc616378a14a0c311533930a9550e0b5aa40ca962e3cc2c50635bc2`。这是 dirty 工作树的本地验证产物，没有覆盖全局安装或操作用户真实 session。
- `PAGER_BINARY=<本次 target/debug/grow> RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=4 cargo test --locked -p pager --test pty_e2e_persistence suspend_nested_task_tree::ctrl_d_notification_turn_suspends_and_resumes_background_child -- --ignored --exact --nocapture --test-threads=1`：最终 1 passed、0 failed、0 ignored，9.93s。
- 同次编译的 `target/debug/deps/pty_e2e_persistence-95d7a50899a6d76c`，同一 PAGER_BINARY，精确执行 `minimal_ctrl_d_notification_turn_suspends_and_resumes_background_child`：1 passed、0 failed、0 ignored，17.16s。
- 同一测试 runner、同一 PAGER_BINARY，精确执行既有 `ctrl_d_suspends_and_cold_resumes_root_child_grandchild_tree`：1 passed、0 failed、0 ignored，12.35s。直接 runner 执行避免并行 Cargo 持有 artifact lock，不换测试代码或依赖。

## End-to-end evidence

普通/minimal 均使用隔离 HOME、临时工作区、loopback provider。后台 B 的命令先启动；后台 A 的文件门控直到 parent 原用户 Turn ended 才解除。A 完成后，parent 出现 `origin=subagent_completed` 的未结束 Turn、live 文本并抵达受控 provider terminal gate，此时 B 仍未结束。

首个 Ctrl+D 后出现 `again to quit`，Runtime 事实数量未增加且 phase 仍为 Running；第二个 Ctrl+D 确认退出，root/B 都为 Suspended，operation 相同，B checkpoint 包含在 root tree，二者原 Turn 没有取消或结束，B 没有终态结果。

新进程 `--continue` 后，原 B task ID、child session ID、Turn ID 和 Spawn 均保持；parent 原通知 Turn 及 B Turn completed，A/B 各一个 completed 结果且 result_ref 指向原 child。原用户输入和派发结果各一次；B 的中断 command settlement/tool_result 各一次，命令启动标记只有一行，未重放原命令。

## Historical binary counterexample

使用同一新测试 runner，PAGER_BINARY 指向本地缓存的旧发布文件 `~/.grow/downloads/grow-2.5.5-macos-aarch64`（`grow 2.5.5 (5998735)`，SHA-256 `4328ce32f9e7a0af35d675bcf45cd39e8d0762a1a13a6ef2d41b4552e2261675`），精确运行普通后台通知场景：预期失败，0 passed、1 failed，12.37s。断言位置在确认退出后的 runtime 边界：实际 Running，预期 Suspended。没有回退或修改当前生产源文件；旧程序确实在同一通知触发条件下退出而未挂起。

## Fixture failures and corrections

新场景前两轮在 B 启动前失败：旧 PTY `responses_api_tool_call_events` 及 parallel helper 都在 function item 宣告前发送 arguments delta，严格 parser 报 `arguments delta without a function item`。它们此前主要经 Chat twin 运行，不能作为合法 Responses 流。最终改用现有 `test_support::sse::responses_api_reasoning_then_tool_call_events`，保留严格生产校验，不过滤坏事件或增加容错。失败结果不计入挂起/恢复通过数；修正后两种模式均完成完整链路。

## Review and static checks

- 独立有范围对抗性审查核对 provider 请求排序和通知/完成门控；按审查补上首击的可见确认与 Runtime 事实不变、provider terminal gate 和独立 subagent_id 断言。
- 相关源文件 rustfmt check 与 `git diff --check`：通过。
- 前序 change 的 UI 回归已有旧 guard 下失败、修复后通过的证据。本轮额外补跑同一 UI 单测时持续等待并行全库测试占用的 Cargo artifact lock，未进入编译/执行，取消了该补跑；不计作本轮通过项。本轮普通/minimal 真正按键、可见首击确认、挂起 ACK 和新进程恢复均已端到端验证，不把未执行的补跑作为门禁证据。
- 首次全仓 `cargo fmt --all -- --check` 因并行工作中的 Timeline、Pager transcript reclamation、consultation 变更未格式化而失败；没有替其他工作格式化文件。本轮新增测试文件检查独立通过。
- 归档前全量 OpenSpec strict 最近一次：20 passed、0 failed；本 change strict 通过。
- 清理了本轮临时 baseline 文件和 PTY incremental（约 62 MiB）；保留已验证 CLI、测试 runner 及其他并行工作仍在使用的共享构建缓存。
- 已归档并合入两个 capability 的对应场景。归档后全量 strict：19 passed、0 failed；archive strict：802 passed、0 failed。
