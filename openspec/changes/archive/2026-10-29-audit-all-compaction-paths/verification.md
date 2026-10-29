# 审计验证

环境：当前 macOS 工作区；Rust 1.93.1；`--locked`、CARGO_INCREMENTAL=0、CARGO_PROFILE_DEV_DEBUG=0、CARGO_BUILD_JOBS=4、RUST_MIN_STACK=16777216。

首次运行 `cargo test --locked -p shell --lib adversarial_audit_ -- --test-threads=4 --nocapture`：构建成功（3m05s），六个预期不变量全部失败（0 passed / 6 failed / 4379 filtered，0.66s）。这次红测是缺陷复现，不是通过记录。输出保存在 probe-results.txt。

探针以新增块临时挂载；已逐块精确撤下并保存完整 reproduction.patch，未还原或覆盖本轮之前的其他工作区修改。修复与正式回归由独立 fix-compaction-audit-counterexamples 记录。重放旧失败 patch 应在该审计当时的源码上进行，不可叠加到已经移植回归的修复源码。

本轮未在线重放用户 session、未执行历史工具、未安装或替换运行中的 Grow。

审计和修复的必要验证结束后已执行 cargo clean，删除 11,620 个文件、4.5 GiB。归档前严格全量 OpenSpec 校验 18 passed、0 failed。

归档后：严格全量校验 `Totals: 16 passed, 0 failed (16 items)`；archive 校验 `Totals: 718 passed, 0 failed (718 items)`。确认 target 目录已移除。
