# Verification

## Baseline and historical evidence

基线：`262665f5d091120e07e4bb5ab513e54e26bc96f2`。

- [发布 Core 失败](https://github.com/LordCasser/grow/actions/runs/42372315045/job/128402894954)：Shell 4787 passed、1 failed、13 ignored。失败是 `cleanup_child_seed_requires_exact_parent_terminal_result_chain` 的父删除错误必须包含 child ID 断言，尚未执行后续 TTL 删除/保留断言。
- [同一提交的独立 Core](https://github.com/LordCasser/grow/actions/runs/42372293484) 成功。
- 原日志没有打印实际删除错误，不能断言已完整还原历史 CI 的具体错误；本次确定性回归独立证明下面的实际实现缺陷。原测试保留关联 ID 断言，并加入实际错误与矩阵条件。

## Deterministic implementation regression

使用生产 `JsonlStorageAdapter` 的公开 `init_session` 和 `append_timeline_event_durable` 入口构建独立小型集成探针。child 只保留原 writer 的继承 descriptor，并通过 ready 文件和 stdin 生命周期保持存活，不创建 writer owner。真实 adapter clone 存活时始终拒绝替代。

旧实现确定性失败（0 passed、1 failed）：最后一个 adapter owner 已 drop，但 child 仍持继承 descriptor 时，替代 append 返回：

`WouldBlock: session lease-probe is being written or is part of an unfinished session reclaim`

应用修复后，同一真实存储探针通过（1 passed、0 failed），随后直接运行已构建探针重复 50 次，50/50 通过。

这证明旧实现把锁的寿命错误地交给了同一 open file description 的最后一个 descriptor。正式 JSONL 回归 `writer_lease_release_does_not_wait_for_inherited_descriptors` 另覆盖显式 release、真实 clone 保留所有权，以及 child 退出不能解锁替代 owner。

## Local environment limit

Linux、Rust/Cargo 1.93.1、约 10 GiB 内存，debug info 与 incremental 关闭，测试线程栈 16 MiB。完整 Shell 单元测试二进制在以下三个配置均于 code generation 被 SIGKILL：CI 的 4 build jobs、1 build job、1 job 且仅 Shell 512 codegen units。没有执行到单元测试，因此不能把编译尝试算作原矩阵、JSONL 或 Shell 全套通过。

小型公共 API 探针单独链接正常生产 Shell 库，避免编译整套约 4800 项单元测试；这是受影响真实入口的验证，不是全套测试替代。探针是临时验证文件，提交中的正式回归仍由 Core 的 Shell 全库测试运行。

## Final checks

- 修复后真实存储入口探针：1 passed、0 failed；随后 50/50 重复通过。
- `cargo +1.93.1 fmt --all -- --check`：通过。
- `git diff --check`：通过。
- `openspec validate --all --strict --no-interactive`：归档前 22 passed、0 failed。
- `cargo +1.93.1 clippy --locked -p shell --lib --tests --no-deps`：被既有 `clippy::never_loop` 阻塞，位置 `agent/mvp_agent/acp_agent.rs:1788`。该文件与基线完全一致，本次未改；其余报告均在未改文件。不将此命令记为通过，不在当前锁生命周期修复中混入独立清理。
- `cargo +1.93.1 check --locked -p shell --lib --tests`：通过，包含正式新增 JSONL 回归与全部可用 Shell 测试目标的类型检查；仅有未改文件中的既有 warnings。
- `openspec validate --all --strict --no-interactive`：归档后 21 passed、0 failed。
- `openspec validate --archived --no-interactive`：923 passed、0 failed。

远端 Core 必须按推送后的精确提交核对，旧 main 的成功不能作为新提交通过的证据。完整 Shell/JSONL/原清理矩阵以及正式继承描述符回归在本地未运行；推送后由 Core 的既有 Shell 全库步骤执行。
