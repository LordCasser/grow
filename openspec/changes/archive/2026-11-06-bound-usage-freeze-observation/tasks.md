## 1. Contract

- [x] 1.1 阅读 OpenSpec 索引、session-timeline/model-sampling 规范与 stall 调查，核对调用方与零预算/取消语义。
- [x] 1.2 建立 change（proposal、delta WHEN/THEN、design、tasks）并通过 `openspec validate --all --strict --no-interactive`。

## 2. Implementation and verification

- [x] 2.1 先写持有 `respond_to` 不回复的失败回归并在当前源上验证 old red（FAILED，`Elapsed`，0 passed / 1 failed / 4692 filtered），记录命令与计数。
- [x] 2.2 按 design 用绝对 deadline 约束 freeze 观察与 error-path 查询；usage-fold 17/17 与 settlement 1/1 green（`--test-threads=1`）；仅对本次触碰的三个文件做 rustfmt check。
- [x] 2.3 记录验证证据（verification.md）；change 保持未归档，交父验收。
- [x] 2.4 按父 review 窄修：`sleep_until` clamp 到原 deadline、helper 改 `timeout_at(deadline.into())`、零预算表述修正、新增晚到回复回归；父最终运行18项用量测试全部通过，见验证记录。
