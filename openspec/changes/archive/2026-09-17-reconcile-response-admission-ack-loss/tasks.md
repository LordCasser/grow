## 1. Canonical response identity

- [x] 1.1 在 chat-state 定义并导出 `ResponseAdmissionIdentity`，给 assistant append `MessageEvent` 增加 optional admission metadata 和 fold 校验；用 Timeline 单元测试验证 matched completed request、错误 attempt、非法 message shape、bulk replay 及 legacy `None`。
- [x] 1.2 将 `PushResponseDurably` 改为 identity-based 幂等 admission：exact items 返回原结果、不同 items typed conflict、duplicate 不重装 native continuation；用 actor 测试验证 Timeline 只保留一份 response。
- [x] 1.3 增加 commit 后 caller reply receiver 丢失的故障注入：在 manual persistence ACK 下丢弃首个 caller，再以同 identity 核对；验证 exact response/repair 恰好一次，malformed response 在返回前完成 quarantine repair。

## 2. Shell fail-closed gate

- [x] 2.1 Shell 用 sampler `request_id + final attempts` 构造 admission identity；当前 ChatState owner 的 acknowledgement/persistence/causal 错误统一映射为 typed `response-admission` turn-boundary failure，不通过失效 owner 重试；cold/replacement owner 可用后由 ChatState exact reissue 幂等核对。
- [x] 2.2 以 compositional regression matrix 覆盖 completion-requirement fail-closed 行为：actor 真实 commit→caller reply loss 与 exact reissue、Shell admission-error fatal wrapper；在现有 harness 不支持 owner replacement 的范围内，未声称端到端 provider-count/Accepted/tool 注入证据。
- [x] 2.3 覆盖 identity conflict 和 actor unavailable：两者均保持既有 Timeline，停止 provider recovery 与工具；运行现有 response quarantine、attempt preview 和 turn-boundary fatal tests 防回归。

## 3. 文档、验证与磁盘

- [x] 3.1 更新 `docs/development.md`，说明 response admission identity、local reconciliation、native continuation 与 cold-recovery 边界；校对 `session-timeline` delta。
- [x] 3.2 创建 `verification.md`，记录故障注入、受影响 chat-state/shell 定向测试、Rust 格式、`git diff --check` 与 `openspec validate --all --strict --no-interactive` 的最终结果。
- [x] 3.3 所有 Rust 验证结束后检查 `target/` 与文件系统占用并执行 `cargo clean`；记录清理前后结果，完成独立 review 后再归档 change。
