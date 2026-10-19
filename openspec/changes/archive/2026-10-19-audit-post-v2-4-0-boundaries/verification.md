# 验证记录

## 环境和范围

- macOS / aarch64，本地固定 HEAD `e7b422990830eee87d68051ed541ecb42f34dffe`。
- 使用仓库固定工具链 Rust/Cargo 1.93.1；显式将 `/Users/lordcasser/.cargo/bin` 放到 PATH 前，避免环境中的 Homebrew 1.99.1。
- Cargo 采用 `--locked`、`CARGO_BUILD_JOBS=2`、`RUST_MIN_STACK=16777216`。
- 无产品修复；仅在两个生产模块末尾临时加入 `#[cfg(test)] mod audit_post_v240`，通过 include 引入本目录探针。执行后移除全部临时入口。

## 定向反例

探针调用实际生产处理函数和既有 actor 测试支撑，不复制算法。失败断言表达应满足的契约，预期用当前代码复现失败；它们不是本轮应修复并转绿的实现任务。

| 探针 | 检查的实际边界 | 结果 |
| --- | --- | --- |
| `audit_v240_cursor_reconnect_merges_image_operation` | Started → begin reload → Completed → cursor success/adopt；同一 operation 必须只有一行且已终止 | 失败并确认 F2：实际 2 行，旧行 running=true，新行 Completed/running=false |
| `audit_v240_child_routes_image_recovery` | 对已有 child view 调用 child 通知处理，必须产生 operation 行 | 失败并确认 F3：changed=false，图片恢复行数为 0 |
| `audit_v240_manual_behavior_keeps_deferred_candidate_durable` | Deferred → 确认切换 Normal 成功后的 latest Control 必须与内存保留同一候选 | 失败并确认 F1：live=true，durable=false；最新快照候选为 None |
| `audit_v240_manual_behavior_preserves_plan_identity_counter` | 完成 Plan → Ask 成功后的 latest Control 必须与内存保留同一提交计数 | 失败并确认 F1：live=1，durable=0，其余快照字段一致 |

Pager 探针保存于 [regression-probes.rs](regression-probes.rs)，临时接入 `crates/codegen/pager/src/app/acp_handler/session_notification.rs`。Shell 探针保存于 [plan-identity-probe.rs](plan-identity-probe.rs)，临时接入 `crates/codegen/shell/src/session/actor/tool/mod.rs`。归档后复现时，include 指向本归档目录对应文件，并在结束后移除临时模块。

运行命令：

```sh
PATH="/Users/lordcasser/.cargo/bin:$PATH" CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 \
  cargo test --locked -p pager --lib audit_v240_ -- --nocapture --test-threads=1
PATH="/Users/lordcasser/.cargo/bin:$PATH" CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 \
  cargo test --locked -p shell --lib audit_v240_ -- --nocapture --test-threads=1
```

首次 Pager 编译暴露探针 fixture 的类型错误：child map 需要 `Box<AgentView>`。修正探针为 `Box::new(child)` 后重跑；该编译错误不属于产品缺陷。Pager 正式反例执行为 0 passed / 2 failed / 0 ignored，Cargo 退出码 101，与要验证的两个缺陷一致。

Deferred 候选探针的初版未执行离开已有 Plan 所需的二次确认，因此先失败在 Applied 断言；改选 Ask 也需要同一确认。最终探针明确断言首次 ConfirmationRequired，再重复 Normal 请求完成确认，只有随后 latest Control 的比较才用于判断持久化缺陷。确认门槛正常生效不属于缺陷。

Shell 最终反例执行为 0 passed / 2 failed / 0 ignored，Cargo 退出码 101；两次都已越过成功的 Behavior 切换，失败点分别是 retained_plan 丢失和 last_plan_submission_id 回退。与 Pager 合计四个反例确认三个独立功能问题。

## 对照与静态核对

- Luna 子审查运行 `python3 -m unittest discover -s scripts/tests`，16 项通过，并核对 same-SHA reusable workflow、精确测试匹配与 release publish 依赖。
- `git show v2.4.0:.../session_mode.rs` 及旧 `behavior.rs` 确认 F1 的空白快照写入在 v2.4.0 已存在。
- Control 写入 → latest_from_timeline → storage load → coordinator restore 的实际调用链与正常挂起补写快照均已逐处核对。
- F4 通过 workflow path 集合与 `.cargo/config.toml` 的 Windows target 配置静态核对。未执行 hosted Windows、macOS/Linux matrix 或 GitHub 发布。
- 使用本轮刚生成的 `target/debug/deps/pager-863ec614a0382641` 执行 `image_recovery --skip audit_v240_ --nocapture --test-threads=1`：6 passed / 0 failed / 0 ignored。覆盖普通重连保留、两种 terminal rail、replay EOF、行渲染、顺序 replay 更新和离线 snapshot EOF。这些通过的场景与新反例不同，未把全部图片恢复判定为失败。
- 使用本轮刚生成的 `target/debug/deps/shell-ffd1faffdad3c39f` 执行 `session::actor::tool::plan_finish_projection_tests --nocapture --test-threads=1`：5 passed / 0 failed / 0 ignored。覆盖手动切换时清除 completed、完成后刷新命令、artifact 缺失恢复、存储/Control 失败保留执行，以及 Plan control lifecycle 文本。

## 未覆盖范围

本轮没有运行全 workspace 测试、真实 provider/OCR、交互式 TUI/PTY、实际进程 crash/restart、长历史性能基准或发布流水线。Pager 反例覆盖状态投影交错；Plan 反例检查 actor 确认写入的最新 Timeline Control，再通过代码链核对冷恢复后果，不将其称作真实断电恢复实验。未发现其他确定性缺陷不等于这些范围都已通过。

## 收尾

- 临时产品源码入口全部移除。执行 `cargo clean --profile dev -p pager -p shell -p tools -p agent`，成功移除 3,772 个文件、11.0 GiB debug 构建残留；不清理 release-dist 产物。
- `git diff --check`：通过；两个临时接入的生产源码文件均无 diff，工作区仅剩 backlog 与本审计 change。
- 归档前 `openspec validate --all --strict --no-interactive`：17 passed / 0 failed（含本 change、既有 MCP 提案及 15 份主规范）。
- `openspec archive audit-post-v2-4-0-boundaries --skip-specs --yes`：成功归档，未更新主规范。归档时任务 4 含归档后校验，故保持未勾选，待实际完成后更新。CLI 提示 proposal Why 过短，已在归档记录补充审查目的。
- 归档后全量严格验证：16 passed / 0 failed。首次 archived 校验为 625 passed / 1 failed，唯一失败为本 change 的任务 4 尚未勾选；实际完成归档和校验后更新任务并重跑。
- 最终 `openspec validate --archived --no-interactive`：626 passed / 0 failed。最终工作区只有 `openspec/backlog.md` 与本归档目录变更，没有产品源码或主规范变更。
