# 验证

- `RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib explicit_delete_is_independent -- --test-threads=1`：旧实现 2 项全部失败，分别是合法 >16 MiB journal 和 300 个完整有效 sampling chunk（实际合计 600 bytes）的 maintenance budget 错误；修复后 2 通过。bounded observer 在修复后仍拒绝预算外读取。
- 最终 `RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib session::storage::jsonl::tests -- --test-threads=1`：159 通过。另含缺失历史 sampling body 的显式删除、checksum 篡改与 checksum 正确但 TurnEnded 无 TurnStarted 的因果损坏拒绝、原 writer/quarantine/cross-session/TTL/复制回归。损坏时 journal 与目录保持原样。
- 不改全局常量；per-record 上限不变。显式删除关联扫描复用 canonical 验证并跳过已经核对的目标；维护扫描仍使用自身发现预算。
- `git diff --check`：通过。所有 fixture 使用临时 root，没有删除用户 session。

OpenSpec：归档前 17 通过，归档后 16 通过，archived strict 772 通过。
