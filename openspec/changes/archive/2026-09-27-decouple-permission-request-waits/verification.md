# 验证记录

日期：2026-09-27。工作树含既有未提交修改，本次保留它们；未提交、构建发布二进制或发布版本。

## 修复前证据

先加入 `child_permission_waits_do_not_serialize_independent_work`，在修改 manager/worker 之前执行：

```sh
cargo test --locked -p shell --lib child_permission_waits_do_not_serialize_independent_work -- --test-threads=1
```

失败：`child A must not block a local primary permission decision: Elapsed(())`。A 已到达真实 HTTP mock provider 并被 gate 挂起，主会话仅申请本地 Read；它在 1 秒内都无法完成，证明共享 manager 的外部等待阻塞了无关本地决策。

修复后的同一测试同时覆盖没有 Goal 和 active Goal 的场景。Goal 场景显式持有一个尚未返回的主模型用量 attempt，然后运行真实 permission manager → classifier channel → Sideband → mock HTTP 路径。要求 A 尚未释放时 B 的 HTTP 裁决结束、主会话 Read 完成；关闭合成主 attempt 后，下一次前台准入也不能等待 A。测试通过，整个用例约 0.5 秒。

20 秒场景是人为推进虚拟时间的 mock，不是生产延迟测量。现在以默认 30 秒完整期限验证：20 秒有效响应成功；31 秒仍不回应则 fail closed；两者均只发出一个 provider 请求，没有为了预留重试而中途重发。

## 最终测试

| 命令 | 结果 |
| --- | --- |
| `cargo test --locked -p workspace --lib permission:: -- --test-threads=4` | 399 passed / 2 failed；两项已在原始 manager 上复现，见下文 |
| `RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib permission -- --test-threads=4` | 80 passed |
| `RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib session::actor::goal_support::tests -- --test-threads=4` | 20 passed |
| `RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib session::actor::sideband::tests -- --test-threads=4` | 18 passed |
| 对本次修改的 Rust 文件执行 `rustfmt --check --edition 2024` | passed |
| `git diff --check` | passed |
| `openspec validate --all --strict --no-interactive`（归档前） | 16 passed |

Shell 扩大到 permission 全组的第一次运行未设置项目开发指南规定的测试栈，在 `injected_write_is_reviewed_per_call_while_matching_edit_keeps_fast_path` 栈溢出；按开发指南使用 16 MiB 测试线程栈后 80 项全过。没有修改生产栈或异步业务调用链。链接器仍提示已有的大型 `__eh_frame` 警告。

## 场景覆盖

- 独立外部等待：A 的 provider 被 gate 阻塞，B 和主本地判断仍完成；同 scope 的两个 prompt/classifier 可以同时进入等待。
- 最新状态提交：两个 prompt 逆序允许后授权均存在并落盘；两个 Block 逆序完成时连续/累计计数为 1、2。
- 权限隔离：父 remembered grant 不授予 child；child 与 sibling 每个精确调用仍需独立 prompt；child 只提供 allow-once/reject-once。
- 控制撤销：Reset/ReleaseChild 撤销在途 classifier；Reset/ReleaseChild/SetMode 撤销旧 prompt；旧 UI 回应不能产生授权；主 mode 更新不取消或放行仍在等待的 child。
- deny 收紧：新的 remembered deny 取消同域旧允许请求，迟到 UI allow 不进入内存或权限文件。
- 关闭：原有 shutdown 测试改为两个真正挂起的分类请求，保留两个 Cancelled 审计事件及 ACK 后 audit EOF 断言。之前的第二个本地请求必须排队的假设不再成立。
- Goal：活跃前台与权限后台并存；已返回、已认领结算、旧 epoch、替换/关闭 Goal 仍受约束；原有取消、exact settlement、一次计费和丢失 ACK 测试通过。
- timeout/retry：完整默认 30 秒、Sideband 准备挂起、20 秒首次有效响应、31 秒超时、实际无效结果/可恢复错误最多重试一次均通过。
- 原有 deny policy、保护编辑、MCP、shell 风险、能力围栏和 exact permit 测试保留执行。

## 原有失败的独立复现

暂时仅把 `manager.rs` 恢复为 HEAD `5a36c7693bd0f13d8eaddd17acbdb4789a200d37` 的内容，用 `try/finally` 在运行后恢复本次文件，再运行：

```sh
cargo test --locked -p workspace --lib permission::manager::tests -- --test-threads=4
```

结果：162 passed / 2 failed，与本次最终权限套件相同的两项：

- `auto_mode_llm_transcript_block_on_real_gate`：断言拒绝消息包含 `exfil`，实际为固定拒绝说明。
- `auto_classifier_block_denies_then_escalates_to_prompt`：断言拒绝消息包含 `reaches beyond the machine`，实际同上。

未删除/跳过断言或修改拒绝行为，差异已登记 `openspec/backlog.md`，另行定义 reason 展示/传播契约。本次纯状态选择 helper 的旧测试已替换为经过实际 manager/prompt 的同名隔离场景，没有只测试一个不再用于生产的 helper。

## 归档和资源清理

- `openspec archive decouple-permission-request-waits --yes` 成功，3 项新增要求、1 项修改要求合入主规范。
- 归档后 `openspec validate --all --strict --no-interactive`：15 passed。
- `openspec validate --archived --no-interactive`：532 passed。
- 确认当前 workspace 无 Cargo/rustc 构建进程后运行 `cargo clean`，删除 55,624 个构建文件，Cargo 报告释放 15.6 GiB。
- 最终 `git diff --check` 通过。
