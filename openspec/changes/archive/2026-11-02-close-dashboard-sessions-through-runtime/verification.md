# 验证记录

## 客户端闭环

- `cargo test --locked -p pager --lib close -- --test-threads=1`：首次 106 通过、5 失败；失败均为旧的同步移除预期。按新 ACK 契约调整这些既有用例后 111 通过。最终补充启动 owner 与 transport 丢失用例后重跑，112 通过。
- `cargo test --locked -p pager --lib dashboard -- --test-threads=1`：发现 compact 停止路径遗漏本地 CommandCancelling 转换，恢复原 foreground cancel 分派后 677 通过、2 ignored。
- 直接运行同次生成 Pager 测试 binary：关闭 ACK effect、最新未同步草稿、关闭后重开草稿、写失败后重开重试各 1 通过。effect 断言真实 ACP 消息与延迟 ACK；客户端确认前保留 view。
- `git diff --check`：通过。

## 服务端

使用 `/tmp/grow-background-stop-shell-control.txt` 中记录的当前 Shell 测试 binary，`RUST_MIN_STACK=16777216`：`explicit_close` 2 通过、`session_lifecycle_gate_serializes_close_behind_load_incarnation` 1 通过。验证真正 Shutdown、终态清理、panic writer 与 load/close 串行化。没有操作用户真实 session。

默认 `cargo build --locked -p cli --bin grow` 失败于本机 jemalloc 生成头 `jemalloc_internal_defs.h` 缺失；清理本次失败的单个 build output 重建仍同错。采用已有 `--no-default-features --features sandbox-enforce`，只切换 allocator、保留 sandbox，构建通过。`GROW_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow RUST_MIN_STACK=16777216 cargo test --locked -p shell --test test_built_binary_e2e test_stdio_session_close -- --ignored --test-threads=1`：1 通过，验证真实 stdio 创建后关闭、runtime 来源消失且 durable history 仍可读取。不修改依赖或绕过 sandbox。

## 覆盖边界

本轮不声称解决跨客户端进程退出 owner 作用域、并发 exit/close writer drain 或恢复 source 准入；这些是已识别、待后续独立增量完成的问题。关闭是 terminal runtime close，保留 durable history；未增加删除或递归清理语义。

## OpenSpec

归档前 `openspec validate --all --strict --no-interactive`：17 通过。
归档后全量 strict：16 通过。
归档任务检查 `openspec validate --archived --strict --no-interactive`：769 通过。
