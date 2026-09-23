# Verification

## Scope and evidence

2026-09-22 在共享 dirty 工作树上复核；不覆盖已有通信、采样、工具权限及其他 changes 的修改，也不归档已有 `reconcile-response-replay-projection`。

- 子会话路径：`replay_inherited_updates` → `stream_replay_updates_at` → `with_reconciled_replay_lines` → `OpenedSession::timeline_events`。缺失 Timeline 会在 ACP 投影前返回 `InvalidData("mandatory Timeline ledger is missing")`。10 项失败均有 Summary/updates 而缺少该文件。
- 工具 UI 行属于普通 ACP history，本组夹具没有 response admission identity；合法空 Timeline 足以供现有 reader 校验。具有 Spawn/receipt 的其他夹具仍保存对应真实事件。
- 首条输入路径：SessionLoaded 关闭 replay gate，取走 `pending_first_prompt`，加入队首，再 `maybe_drain_queue`。`combine_queued_prompts` 为真时两条普通输入合并为一次 effect；原测试采用本机配置但只断言“不合并”的队列状态。
- 空历史用例原用 `{}` Summary，会因无效会话而提前返回。修复为合法 Summary + 空 Timeline + 空 updates，并显式确认 reader 返回 `ReplayEmission::Empty`，再断言不释放内存。

## Before repair

隔离编译：`CARGO_TARGET_DIR=/tmp/grow-pager-recovery-target CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo test --locked -p pager --lib --no-run`。

本轮修改前编译的 Pager test binary 整包运行（4 threads）：**7,230 passed / 11 failed / 11 ignored**，21.06 秒，失败集合与 backlog 完全一致。日志：`/tmp/grow-pager-recovery-before.log`。该结果是当前共享工作树的修复前证据，不是干净 HEAD 构建。

## After repair

Pager 整包：**7,242 passed / 0 failed / 11 ignored**，20.64 秒。原 11 项失败全部通过，新增合并开启场景通过；两种首条输入场景都验证发送 effect 恰好一次、in-flight 内容、FIFO 与 pending 字段清空。日志：`/tmp/grow-backlog-shell-pager-final.log`。

命令：`CARGO_TARGET_DIR=/tmp/grow-pager-recovery-target CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo test --locked -p shell -p pager --lib -- --test-threads=4`。

实际工具链：Homebrew rustc/cargo 1.98.1。针对本次 6 个 Rust 文件的 `rustfmt --edition 2024 --config skip_children=true --check` 和 `git diff --check` 均通过。没有运行新的 PTY 或性能测量；11 个 ignored Pager 测试没有作为已验证结果。

独立 review 确认空 Timeline 是合法 ledger、原 transcript/release/notice 断言保留、combine cache 为线程局部且使用 RAII 恢复。review 提出的发送计数缺口已补上并由整包验证。

归档前严格 OpenSpec：22 passed / 0 failed。全部 Rust 验证完成后执行 `cargo clean --target-dir /tmp/grow-pager-recovery-target`，删除本轮 9,993 个文件、6.9 GiB；磁盘剩余 26 GiB。没有清理其他任务目录。

已使用 `openspec archive repair-pager-recovery-regressions --skip-specs --yes` 归档；该测试维护变更没有主规范 delta。两项本轮变更归档后，全量严格校验 **20 passed / 0 failed**，archive 校验 **356 passed / 0 failed**。日志分别为 `/tmp/grow-backlog-openspec-final.log` 和 `/tmp/grow-backlog-openspec-archived.log`。未执行 Git 提交。
