# Task T · ChatState canonical pending order

## 状态与实现边界

本记录承接 `verification-query-order.md` 的历史测试准备，记录本轮已执行的实施与验证。Task T 已由父核本轮前基线/实际diff、公共查询反例、原validator失败原子性和最终日志，接受模块交付；不代表 Task S、Session F2 或 campaign 完成，无 commit/release/archive。

本轮 Rust 写入只涉及 `crates/codegen/chat-state/src/timeline.rs`、既有 `src/actor/tests.rs` 与已准备的 `tests/edited_fifo_recovery.rs`。工作区原有 canonical Handled/consumption 及其他 dirty 改动保留；生产实现的本轮前基线另存 `/tmp/grow-fifo-order-timeline-baseline.rs`，可用 `git diff --no-index` 区分本轮 Timeline 变化。

`InputFold.canonical_position` 是 private 派生状态：合法 Submitted 以当时保留的输入数初始化序位；输入 fold 不删除历史输入，因此普通新输入序位持续增长。原 AdmissionResolved 校验全部成功后，候选输入继承 supersedes 中最小 canonical_position，再按原规则将旧身份终结为 RouteSuperseded。没有 supersedes 的输入保留 Submitted 序位。连续替换继承同一序位，多 input 替换不取 supersedes 列表顺序。

`pending_allowed_inputs` 直接筛选既有生命周期 fold，按该序位排序后输出原 `PendingAllowedInput`。公共 API、返回字段、事件/schema、payload_ref/input_id、validator 与 Shell 均未改变；无新持久队列或公共排序 registry。Consumed、Handled、Dismissed、Reserved 继续排除，Steer 继续保留其原 route。

prepare 在克隆的 lifecycle 上派生位置且不发布；accept 安装已验证 fold；replay 按原 Submitted/AdmissionResolved 事实重建同一位置。拒绝路径不提交部分位置或部分 RouteSuperseded。

## 实际旧红

环境为 macOS aarch64，Rust `+1.93.1`，`--locked --offline`；命令均在仓库根执行，使用：

```sh
env -u CARGO_TARGET_DIR \
  PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin" \
  RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 \
  cargo +1.93.1 test --locked --offline -p chat-state <以下 target/filter>
```

| target/filter | 原生产查询上的实际结果 |
| --- | --- |
| `--test edited_fifo_recovery` | exit 101；4 tests：2 passed、2 failed。A/B/C 编辑 A 与逆序多 input 替换均在 replayed query 得到 B/C/A′，期望 A′/B/C；未成功 admission 和过滤控制通过。 |
| `--lib pending_allowed_query_inherits_replacement_slots_through_the_actor` | exit 101；1 failed、643 filtered out。真实 handle 提交完成后，公共 query 得到 B/C/A′，期望 A′/B/C。 |

证据 `/tmp/grow-fifo-order-oldred.log` 保存最终有效旧红，末尾为 `timeline_exit=101 actor_exit=101`。初次 Actor 测试使用 TimelineEvent 的直接相等比较而未编译；修正为序列化值比较后重新在未修改的生产查询上执行，最终日志中的两个失败均为预期的顺序断言，不把编译错误计作产品旧红。

## 新绿与回归

修正后的最终源码实际结果：

| 命令 | 结果 |
| --- | --- |
| `cargo +1.93.1 test --locked --offline -p chat-state --test edited_fifo_recovery` | exit 0；4 passed、0 failed：连续 A/B 编辑、逆序多 input、未接纳/Block、Consumed/Handled/Dismissed/Reserved 全部通过。 |
| `cargo +1.93.1 test --locked --offline -p chat-state --lib pending_allowed_query_inherits_replacement_slots_through_the_actor` | exit 0；1 passed：真实 Actor handle 在每次成功 replacement ACK 后返回 canonical 顺序。 |
| `cargo +1.93.1 test --locked --offline -p chat-state --lib timeline::tests::` | exit 0；150 passed、3 ignored：完整 Timeline fold 回归，含新增普通顺序与拒绝原子性测试。 |
| `cargo +1.93.1 test --locked --offline -p chat-state --lib input` | exit 0；26 passed：相关 input、Actor 生命周期与 fixture 回归。 |
| `rustfmt +1.93.1 --edition 2024 --check --config skip_children=true <本轮三个文件>` | exit 0。 |
| `git diff --check -- <两个已跟踪文件>` | exit 0。 |
| `openspec validate --all --strict --no-interactive` | exit 0；21 passed、0 failed，包含本 change。 |

证据：`/tmp/grow-fifo-order-green.log`（初次最终行为 green）、`/tmp/grow-fifo-order-regress.log`（最终源码 fold/input/actor 回归，末尾 `fold_exit=0 input_exit=0 actor_exit=0`）、`/tmp/grow-fifo-order-check.log`（格式与 strict spec）。对未跟踪 fixture 执行 `git diff --no-index --check /dev/null <path>` 返回 1，这是比较两个不同文件内容的正常退出码，输出中没有 whitespace 诊断。

## 覆盖与限制

- integration fixture 经实际 Timeline::record、Hook lifecycle、原 admission validator 构造有效前缀；每次 successful admission 后经 JSON 往返和 Timeline::from_events 重建并继续录入，不直接注入私有 fold。A 两次/B 一次的每个切点核对顺序、新身份/payload_ref、旧 RouteSuperseded；新普通输入保持队尾。
- 逆序 supersedes 多 input 替换继承最早位置，再编辑继续继承。Submitted 未决、Hook Allow 但仅 prepare、真实 Hook Block 均保留旧 Allowed A；Consumed、Handled、Dismissed、Reserved 的替换在 replay 后仍不 pending。
- 公共 Actor query 复用现有 TestHarness 与 durable input helpers；每次 replacement ACK 后断言返回顺序、最新 payload_ref 与旧身份终态，并核对查询不追加 Submitted/Hook 事件。
- 同模块控制覆盖反向 admission 的普通 Submitted 顺序、Steer/Fifo route 往返，以及 self、duplicate、missing、superseded、reserved、consumed、handled、dismissed、Steer 目标的原拒绝行为；prepare/record/accept 拒绝后整个 fold/query/events 保持，非法后缀 replay 也拒绝。

payload_ref 是既有 fixture 的 opaque 哈希/长度，没有物化 Shell InputPayload 或图片文件。这里验证 accepted event prefix 的重建和 mock persistence ACK 下的 Actor 公共查询，没有验证真实磁盘 fsync、实际进程冷启动、真实 Shell 编辑/Hook 执行次数、行/runtime prompt identity、图片保真或 unknown ACK 的产品重试。Task S 及父里程碑仍须执行真实 Shell 编辑→持久切点→cold 恢复；未跑其他平台或产品交互。
