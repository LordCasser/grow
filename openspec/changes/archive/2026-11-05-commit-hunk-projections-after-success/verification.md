# Verification

## Accepted result

WS-06b 已验收。hunk action owner 在 single 或每文件 batch 动作成功后才提交统计、turn membership 和 affected ids；失败文件不计成功。多文件 batch 保留此前已成功文件的实际内容、事件和统计，不提供全项目回滚。WS-06a 磁盘版本检查仍由后续独立 change 实施，本项不声称外部编辑已受保护。

仅修改 `crates/codegen/hunk-tracker/src/actor/actions.rs` 与同目录 `tests.rs`。父核对完整 diff、公开 Handle 到 single/file/turn/all 的调用链及 `recompute_hunks`：目标已从 file hunks 移除，重算只重建剩余 hunk 的 turn 索引，外层移除旧目标 ID 不会删除重算身份。合法候选由 actor 串行从当前 state 收集，现有 invariant 排除在缺失或不可 patch 状态上把 NoOp 计成成功的路径。

## Executed checks

macOS；Cargo、rustc、rustdoc 均为 1.93.1，`RUST_MIN_STACK=16777216`、`TMPDIR=/tmp`、独立 `GROW_HOME=/tmp/grow-ws06b-home.RtxPr9`。未安装或重启正常 Grow，未操作真实 session 或用户文件。

1. `cargo test --locked -p hunk-tracker --lib actor::tests::test_reject_io_failure_keeps_projections -- --exact --test-threads=1`：1 passed，0 failed，0 ignored，202 filtered。该 test 内含 single/file/turn/all × 真实 write/delete 故障的 8 组公共 Handle 场景；磁盘故障目标、pending hunk、完整 file state、summary、turn membership 和事件保持。
2. 复用上述 Cargo 实际生成的 `target/debug/deps/hunk_tracker-2c595f815f38e1cd`，先用 `--list --exact` 确认 16 名称集合，再在 crate cwd 以 `--exact <16 names> --test-threads=1` 执行：16 passed，0 failed，0 ignored，187 filtered。包含 partial batch/retry、single/file 正常 accept/reject、剩余 hunk 重算、turn 分组、creation/deletion 和混合 patch 对照。
3. 限定 Rust 1.93.1 rustfmt `--check`、父 `git diff --check` 通过；父重新计算 SHA-256 与运行候选完全一致：actions `4c54ffbc5e3b944879ec8171a10f6aa81bcfc63d71415298b5521e0dd1b056ef`，tests `3d856884be201cf1186d0c02c31ff5d7301130355dee1a00afd2b99941ac7cb3`。

两组运行共 17 个不同 top-level tests。日志由父逐项读回，终态均 exit 0；先前观察超时已由原任务 completed 结果消除。原始终端日志留在 coder `01a1a996-250c-75d1-b070-c366429cba83` 的 `call_f0821aaecf094a5f9577b2ebf9ddf14e.log` 和 `call_554fd053faf24ad3a2fefd92adc93f80.log`，归档结论不依赖临时产物仍存在。

## Independent review and evidence limit

独立 reviewer `magpie/gpt-6-luna` / `01a1a99d-7f2a-7971-80e2-f36c2d715c50` 静态核对最终两文件 diff、动作/重算/错误边界，未发现本项生产 blocker。reviewer 未重复运行 Cargo；运行证据由 coder 执行、父检查原日志与最终 hash。

review 指出 turn 的独立 HashMap 可能先处理故障文件，所以该 test 不保证 turn 每次都发生“至少一文件先成功”。父接受组合证据：公共 all 入口确定性实现两文件先成功、最后失败并重试；file/turn/all 均直接进入同一个 `apply_action_batch`，没有分支性的提交或恢复逻辑；turn 的真实失败/重试和正确分组由另外的公共 Handle 用例覆盖。因此部分提交规则已由实际公共路径验证，未为控制无语义要求的顺序引入产品排序或测试注入框架。不得将结果描述为 turn 入口确定性覆盖了部分成功顺序，也不声称跨文件事务性。

本项特性验收通过；数据完整性里程碑还须在 WS-06a 加入 stale error 后验证同一失败投影，并核对 Shell 错误传播。其他审计事项继续保留。
