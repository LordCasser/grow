# 验证记录

## 现场

只读对照 session `01a1a0fc-f83a-70e0-aefa-339ca4f054b4` 的不可变 input payload 与物理 seq 2932：用户正文 128563 UTF-8 字节，permission_evidence 与原文完全相同，模型 content 只有 25090 字节且不含原文最后 100 字节。没有把完整显示/权限证据误当成模型正文。

## 自动验证

- 在仍含硬截断的 formatter 上先加入长 UTF-8 中后部指导回归，用 rustup 1.93.1 `rustc --edition 2024 --test crates/codegen/tools/src/interjection/format.rs -o /tmp/grow-long-interjection-regression` 编译，执行 `tests::preserves_long_utf8_text_and_trailing_guidance --exact --nocapture`：1 failed，MIDDLE_GUIDANCE 被截掉（预期失败）。
- 删除截断后同一 formatter standalone suite：3 passed，逐字核对完整 envelope，中部及尾部指导保留。
- `cargo test --locked -p shell --lib interjection -- --test-threads=2`：17 passed。新 `long_utf8_steer_survives_admission_consumption_request_and_cold_replay` 经过实际 steer 准入、原 identity 的单次 durable consume、provider request 构造、display echo 与 permission evidence 核对及 Timeline 冷重放。既有自动 followup promotion、工具结果隔离、FIFO、终态回收与迟到 steer 拒绝回归通过。
- `cargo test --locked -p shell --lib planning_ -- --test-threads=2`：33 passed，包含重复阻塞停止与另一项现场验收修复。
- `cargo test --locked -p tools --lib interjection -- --test-threads=2`：17 passed，包含共享 FIFO buffer 和格式化保真回归。
- 本次涉及文件的 rustfmt 检查和 `git diff --check` 通过。全仓 fmt 检出其他并行/既存改动，未扩大本 change；清单见同次交付验收修复记录。
- `openspec validate --all --strict --no-interactive`：归档前 19 passed、0 failed。
- 两项 change 归档后：全量严格校验 17 passed、0 failed；archive 校验 824 passed、0 failed（完成归档收尾 checkbox 后重跑）。

## 覆盖边界

本次证明长插话从接纳到请求构造及恢复不再静默丢正文，不代表任意模型接受任意长度请求；现有 context pressure 准入继续生效。没有线上 provider 成功率测量或 TUI 像素回归，不修改真实历史中已截断的 content；完整原文仍在原始 payload。普通 prompt export 生命周期疑点已登记 backlog，独立复现后处理。
