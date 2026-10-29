# 验证记录

日期：2026-10-29。本轮只修复重复提升事实；此前工作区中的压缩、Plan、Pager 等修改保留。

## 可复现反例

`compaction_repromotion_tests` 在修复前 **0 passed / 2 failed**，两项均报重复 Promoted 的 CompactionNotOpen，并升级为 `turn_boundary_persistence_failed`。见 [原始失败](probe-before.txt)。修复后两项均通过，且断言只发出一次 summary provider 请求、只有一个 Promoted，真正取消时无 Completed 或摘要 replacement。

## 最终回归

环境：Rust 1.93.1，`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 RUST_MIN_STACK=16777216`。测试二进制通过 `cargo test --locked -p shell -p chat-state --lib --no-run` 构建；随后直接执行 Cargo 返回的路径，避免构建阶段与测试阶段混淆。

| 验证 | 结果与证据 |
| --- | --- |
| ChatState 全量 library | **594 passed / 0 failed / 3 ignored**，包括 8 项新 ACK/提升回归；[输出](chat-state-regressions.txt)。最终命令为 `cargo test --locked -p chat-state --lib`，忽略项沿用原测试标记。 |
| Shell `compaction` 过滤的全部 library 回归 | **114 passed / 0 failed**，4 test threads；[输出](compaction-regressions.txt)。涵盖现有后台、promotion deadline、model-switch、pre-prune、provider overflow、摘要适配与验收路径。 |
| waiter drop / 真正取消后重新提升 | **2 passed**；[输出](repromotion-results.txt)。 |
| 同窗口切 route / 降窗 replacement 取代待发布通知 | **2 passed**；[输出](route-notice-results.txt)。前者核对实际新模型请求及通知 tokens_after，后者要求合法选区压缩成功，未放宽成接受超窗错误。 |
| replacement 后 Control ACK 丢失及修复失败 | **1 passed**；[输出](control-fault-results.txt)。核对一次摘要调用、精确重试、后继写入被拒绝以及冷恢复的摘要、AgentRole、唯一 Completed。 |
| 相关 Rust 文件 rustfmt 检查、git diff --check | 通过；使用 `skip_children=true` 避免格式化无关模块。 |

本轮新增 13 项测试，其中 stale promotion 一项分别覆盖不同 ID、Summary 已提交、事务已终止三个分支。独立复查确认 actor 串行 ACK、generation 所有权和严格 Timeline 校验仍保留。

## 测试夹具校正

初版同窗口路由用例因 mock 的请求分类未命中而超时，但真实 turn 已成功结束；改为匹配该最小 catalog 的请求类别，并明确断言请求模型和人工输入。Control 注入夹具初版使用不合法的 user_meta，随后又因最近 verbatim tail 太短未选中来源；最终使用合法 system_reminder 和既有选区策略所需的尾部，再验证确实到达两次 reprojection ACK 尝试。均未修改产品校验或放宽失败断言。

## 范围

ACK 故障通过受控 TimelinePersistence 内存 sink 注入，恢复通过真实 Timeline projector/settlement 执行。这验证 actor/事务边界，不代表真实文件系统断电测试、线上 provider 或 GUI 实测。本轮未发布二进制，也未修改独立 Plan handoff backlog 项。

生产构建、规范归档和清理结果见下方收尾记录。

## 本机生产构建环境修正

首次默认 CLI check 在 `tikv-jemalloc-sys` 的 C 配置阶段失败：`jemalloc_internal_defs.h` / `jemalloc_defs.h` 未生成。不是本次 Rust 修改的编译错误。[完整日志](production-check-initial.txt)。

定位到 `PATH` 优先选中了 DevEco SDK 的 `diff`。对“缺失文件 vs 存在文件”的同一比较，该工具返回 0，macOS `/usr/bin/diff` 返回 2，导致 configure 把不存在的头文件误判为 unchanged。本轮只清理该 crate 的失败构建产物，并在当前 check 进程将 `/usr/bin:/bin` 放在 SDK 工具前；未改全局环境或项目依赖。

## 收尾

- 优先使用系统工具的 `cargo check --locked -p cli --bin grow`：通过（默认生产 features，24.21s），[日志](production-check-passed.txt)。没有通过关闭 jemalloc 或启用测试 feature 绕过生产图。
- `openspec validate --all --strict --no-interactive`：归档前 17 项通过、0 失败。
- 对应实现、调用方、13 项新增测试与以上场景逐一核对完成；既有忽略测试未改动。
- `openspec archive verify-compaction-publication-boundaries --yes`：归档成功；合入 1 个新增 requirement、2 个修改 requirement。
- 归档后 `openspec validate --all --strict --no-interactive`：16 passed / 0 failed；`openspec validate --archived --no-interactive`：719 passed / 0 failed。
- `cargo clean`：删除 13,547 个构建文件，回收 3.5 GiB；未清理其他项目或进程的产物。
