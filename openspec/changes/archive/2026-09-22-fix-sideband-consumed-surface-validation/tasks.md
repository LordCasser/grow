## 1. Reproduction and canonical validation

- [x] 1.1 用故障会话的只读账本证据固定：压缩 attempt 含合法 consumed-input/notification Surface IDs，失败与 foreground/subagent activity 无关。
- [x] 1.2 在 Timeline owner 提供 canonical Surface coordinate query，并让 Sideband parent validation 复用它，保留 seq/item/range 的严格拒绝。
- [x] 1.3 增加 producer 与非 producer 单元回归，覆盖 Messages、Input::Consumed、Notification::Consumed、Control、ImageProjection 及越界/非 Surface 事件。

## 2. Reload and coordination regression

- [x] 2.1 增加带 consumed coordinates 的 compaction Sideband 严格 storage reload 回归，证明既有合法 ledger 无迁移恢复。
- [x] 2.2 增加 foreground busy/idle 与后台子 Agent 场景下的 inquiry 回归，证明 receipt、Sideband answer 和 terminal audit 不依赖 activity。

## 3. Verification and archive

- [x] 3.1 运行受影响 chat-state/shell 定向测试、必要 package check、changed-file rustfmt、`git diff --check` 与 OpenSpec 全量严格校验，将结果写入 `verification.md`。
- [x] 3.2 核对实际 diff 与场景，归档 change，运行 active/archive 严格校验；所有 Rust 验证完成后执行 `cargo clean`。
