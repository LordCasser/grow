## 1. Canonical projection model

- [x] 1.1 在 chat-state 提供经过 Timeline branch/rewind fold 校验的 admitted-response view；覆盖 active branch、compaction shadow、rewound response 和 legacy identity-less response。
- [x] 1.2 在 Shell 定义纯 response replay projector 和 versioned storage-only record；digest 覆盖 identity、Timeline event、canonical items、quarantine result 和 projector version。
- [x] 1.3 用 projector 单元测试固定 healthy text/reasoning、fallback-only、tool identity ordering、quarantine zero-raw-preview 和 deterministic serialization。

## 2. Durable live gate

- [x] 2.1 通过 session event FIFO 增加 projection barrier；persistence actor 丢弃 exact candidate rows、保留交织独立 updates，并按 key/digest 幂等 durable append 单条 projection record。
- [x] 2.2 调整 turn 顺序：Timeline ACK → fallback candidate/FIFO drain → projection ACK → public Accepted/Discarded → continuation/tool/terminal；projection uncertainty 使用 typed fatal boundary，不能进入 completion recovery 或新 provider request。
- [x] 2.3 增加 commit/ACK-loss/conflict/failure fault tests，证明 projection 恰好一次、candidate 不进入 accepted replay、工具和 continuation 不越过失败边界。

## 3. Reconciled replay

- [x] 3.1 在 storage 建立共享 record scan/validation/expansion 与 missing-projection synthesis；writer repair 和 read-only in-memory replay 使用同一 core。
- [x] 3.2 在普通 session/load 的 replay snapshot/cursor cutoff 前完成 reconciliation，并使 direct child replay、CLI export 等 production reader 使用同一语义；内部-record cursor 只在完整边界增量继续，否则 full replay。
- [x] 3.3 覆盖 cold kill point、resident reconnect、later cursor、direct child replay、discarded earlier attempt、rewind、legacy no-identity、projection conflict 和 healthy tool result ordering；验证 reconciliation 不调用 provider 或工具。

## 4. Documentation, review and verification

- [x] 4.1 更新 `docs/development.md`，说明 Timeline authority、response projection record、candidate/live lifecycle 与 replay reconciliation 边界；校对 `session-timeline` 和 `client-surfaces` delta。
- [x] 4.2 运行受影响 chat-state/shell/pager 定向测试、必要 package check、changed-file rustfmt、`git diff --check` 和 `openspec validate --all --strict --no-interactive`，将最终证据写入 `verification.md`。
- [x] 4.3 独立 review 实际 diff 与 production replay callers；所有 Rust 验证后检查 `target/`/文件系统并执行 `cargo clean`，归档 change 后验证 active 与 archived specs。
