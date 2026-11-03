# 验证

- `RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib independent_fork_delete -- --test-threads=1`：旧实现 2 项全部失败；普通 fork 因 summary ancestry 拒绝，incoming Spawn 无 Seed 的目标却被删除。修复后 2 项通过，覆盖同 cwd/cross-cwd、删 source/删 fork，并读取保留历史。
- 最终 `RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib session::storage::jsonl::tests -- --test-threads=1`：155 通过。含原 canonical Seed/Spawn 的 parent/child 单体拒绝、writer lease、quarantine 身份、TTL 和 fork 内容复制回归。
- `git diff --check`：通过。所有读写使用临时 storage root，不涉及用户真实 session。
- 本轮未解决 A2 的显式删除观察预算、A3/A4 的 runtime/UI 失败收敛或 G1 的挂起历史显式放弃；分别作为后续独立增量，未通过改动全局限制或递归删除绕过。

OpenSpec：归档前全量 strict 17 通过，归档后 16 通过，archived strict 771 通过。
