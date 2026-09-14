## Automated verification

- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p chat-state usage -- --nocapture`：11 passed，0 failed。覆盖主 Agent、两个不同子 Agent、多模型 fold、incomplete 身份占位、累计事件和 attempt 结算回归。
- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib subagent_usage -- --nocapture`：16 passed，0 failed。覆盖 shell → chat-state 的 `subagent_id` 透传、prompt/session-only 归属及既有 incomplete/drain 策略。
- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib extensions::usage::tests -- --nocapture`：2 passed，0 failed。包含子 Agent 的 ledger 被投影为累计总体和 provider/model 分项，序列化结果没有 `agentUsage` 字段。
- `rustfmt --edition 2024 --check crates/codegen/chat-state/src/usage.rs crates/codegen/shell/src/extensions/usage.rs crates/codegen/shell/src/session/actor/updates.rs crates/codegen/shell/src/session/actor/tests/subagent_usage_fold_tests.rs`：通过。
- `git diff --check`：通过。
- `openspec validate --all --strict --no-interactive`：18 passed，0 failed（归档前）。
- `openspec validate --all --strict --no-interactive`：17 passed，0 failed（归档后 active specs/changes）。
- `openspec validate --archived --no-interactive`：首次准确报告本 change 的归档任务仍为 4/5；补记归档后校验结果并勾选该验证任务后重跑，最终结果见下方归档校验。
- `openspec validate --archived --no-interactive`：321 passed，0 failed（最终归档校验）。

## Environment notes

- 首次 shell 定向构建因磁盘只剩 117 MiB，以 `No space left on device` 停止；执行 `cargo clean --package shell` 仅删除 22.2 GiB 可重建 Cargo 产物后，按 `--lib` 精确重跑并通过。
- `cargo fmt --all -- --check` 仍报告工作树中大量本 change 范围外的既有格式差异；没有用全仓格式化覆盖并发未提交改动。上述四个本 change 直接修改的独立 Rust 文件已用 `rustfmt --check` 验证。
- shell 链接器保留既有 `__eh_frame section too large` 警告；测试成功，警告与本次账本数据变化无关。

## Scenario review

- 主 Agent 调用写入 `UsageAgent::Owner`；每个完成子 Agent 按 `UsageAgent::Subagent(subagent_id)` 累计，同一子 Agent 的多个 model row 只向总体和该 Agent 各 fold 一次。
- prompt 可归属与 session-only 两条子 Agent 路径均把同一身份交给 chat-state；既有 ACK、sticky 和 incomplete 所有权不变。
- `PromptUsage` 类型未增加 Agent 字段；`/usage`、headless 与 normal 状态栏继续读取包含全部已结算 Agent 的 `totals`，当前不展示逐 Agent 明细。
- 本次不改变终态 fold 时机；仍在子 Agent 完成前通过既有 incomplete 语义表示可能低估。
