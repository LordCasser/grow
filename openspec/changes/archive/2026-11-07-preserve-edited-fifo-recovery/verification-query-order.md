# ChatState canonical query 准备记录

## 状态与边界

**Staged：测试准备已完成，尚未编译或执行旧红，不代表功能完成。** 本轮按授权只新增：

- `crates/codegen/chat-state/tests/edited_fifo_recovery.rs`
- 本记录 `verification-query-order.md`

未修改生产 `src/`、Cargo 文件、既有测试、主规范、backlog 或本 change 的 tasks；未运行任何 Cargo 命令、未提交 Git、未访问真实用户状态。父当前将 Cargo 唯一窗口交给 Pager U1；后续须由父明确授权独占窗口后再执行 old red → 最小生产修复 → new green。本记录不扩张此次授权。

准备依据为工作区 HEAD `8ab486a6e45870561b91dba872934a6e24a795f1` 加已有未提交改动；`timeline.rs` 在本轮开始时已经有 canonical Handled/consumption 相关改动，本轮保留原状。恢复实施前须重新核对当前源码及窗口状态。

## 实际入口与 fixture 依据

测试只依赖现有公开入口和导出，不要求改 visibility：

| 源码位置 | 已核对事实 |
| --- | --- |
| `chat-state/src/lib.rs` 的 `pub use timeline` | 导出 Timeline、Input/Hook/Turn 事件、PendingAllowedInput、InputState 等本测试所需类型。 |
| `timeline.rs::Timeline::record`、`prepare`、`accept` | `record` 先 prepare 验证再 accept；prepare 不改变已接受 fold，适合验证已准备但未接受的替换。 |
| `timeline.rs::Timeline::events`、`from_events`、`with_replayed_suffix` | 可取完整接受前缀并走实际 replay validator；无须构造私有 fold。 |
| `timeline.rs::Timeline::pending_allowed_inputs`（准备时 2954） | 从物理 Submitted 事件顺序筛选 Allowed 且未 consumption/dismissed/reserved 的输入，尚未继承 supersedes 槽位。 |
| `timeline.rs::LifecycleFold::accept` 的 AdmissionResolved 分支（准备时 5807） | 只有合法 Allow 能携带 supersedes；目标必须是未消费、未 dismissed、未 reserved 的 Allowed FIFO 输入。成功时旧输入转为 RouteSuperseded。Block 必须 route=None 且 supersedes 为空。 |
| `timeline.rs::valid_input_admission`、`HookFold::derived_decision` | Admission 必须符合真实 Hook 结果；空 handlers 的 Prompt Hook 只能自然聚合 Allow，不能伪造 Block。 |
| `timeline.rs::LifecycleFold::accept` 的 Turn::Started / Input::Consumed 分支 | 用户 Turn 通过 input_ids 合法 reservation，Consumed 只能消费所属 active Turn 的 reserved FIFO 输入。 |

复用方式是沿用既有 fixture 的事件构造方法，而非链接其私有 `#[cfg(test)]` helper：

- `timeline.rs` 的 `submit_human_input`、`trigger_user_prompt`、`allow_input_with`：使用实际 Submitted → UserPromptSubmit(Prompt gate/Input cause) → Completed(Allow) → AdmissionResolved(Fifo) 序列。
- `admission_atomically_supersedes_the_previous_queue_identity`：沿用合法 supersedes 和 RouteSuperseded 断言，补多行次序及连续编辑反例。
- `command_handler`、`hook_lifecycle_requires_exact_plan_and_typed_completion`：Block 使用真实 Command handler 的 Triggered → RunStarted → RunFinished(Blocked/Block) → Completed(Block)，再录入合法 Block admission；不执行外部命令。
- `user_turn_atomically_consumes_every_combined_fifo_input`：沿用合法用户 Turn reservation 和 Input::Consumed 的公开事件形状；无需 Step 或伪造 fold。

与既有 `submit_human_input` 一样，fixture 将 input_id 的非空字节作为 opaque payload 的哈希/长度依据；这些 reference 满足 Timeline validator。测试断言最新输入的 payload_ref、intent、route 与身份，但不物化 Shell InputPayload，不声称验证 payload 文件正文或图片 artifact。

## 已准备场景

测试中的期望顺序均为手写 ID 列表；helper 只映射其已知 payload_ref，不扫描 supersedes 推导期望或复制待修排序逻辑。每个查询检查点都序列化完整已接受事件前缀，JSON 解码为真实 TimelineEvent，再经 `Timeline::from_events` 重建；查询同时覆盖 live 和 rebuilt owner，之后继续从重建 owner 录入。还比较 next_seq、序列化事件证据不变及所有 Submitted 身份的 canonical input_state 往返一致。

| 测试 | 验收断言 | 当前实现的静态预期，未执行 |
| --- | --- | --- |
| `successive_edits_keep_a_b_c_slots_at_every_successful_admission_cut` | A、B、C 各次成功接纳后重建；A′ supersedes A、A″ supersedes A′、B′ supersedes B 各次后分别得到 A′/B/C、A″/B/C、A″/B′/C。旧版本始终 RouteSuperseded 且不 pending。 | 应在第一次 A′ admission 后的 `replayed query` 断言旧红：实际 B/C/A′，期望 A′/B/C。 |
| `combined_live_group_inherits_the_earliest_replaced_slot_and_keeps_it_on_reedit` | 初始 A-head/B/A-tail/C；以逆序 supersedes=[A-tail,A-head] 合并，期望 A-combined/B/C，继续编辑保持原最早槽位。所有被替换输入终止。 | 应在合并后的 `replayed query` 断言旧红：实际 B/C/A-combined，期望 A-combined/B/C。此输入批次只表达 Timeline 能验证的多 input replacement；未验证 Shell 的 group 构造。 |
| `unaccepted_and_hook_blocked_edits_leave_the_old_allowed_row_pending` | 只有 Submitted 的候选、Hook 已 Allow 但 AdmissionResolved 仅 prepare 未 accept 的候选、真实 Hook Block 的候选，均不终止旧 Allowed A；每个检查点仍 A/B/C。 | 应作为保持项通过，但未经编译或执行，不能记 green。 |
| `consumed_dismissed_and_reserved_replacements_stay_out_of_pending_after_replay` | 表驱动覆盖 A′ 被 Consumed、UserRemoved 或合法 Turn reserved；重建后只剩 B/C，A 保持 RouteSuperseded。Reserved 的生命周期仍 Allowed，但 query 排除。 | 应作为保持项通过，但未经编译或执行，不能记 green。 |

前两个测试旧红会在第一个错误切点终止；后续连续编辑切点已写入，需修复后实际执行才有覆盖证据。后两个保持项独立于顺序红测，可在未来窗口分别运行。

## 可编译假设与实际检查

已从源码核对的编译前提：所有使用的类型/方法都是现有 public API；测试 target 可直接导入所属库 `chat_state`，`sampling-types`、`serde_json`、`blake3` 均已在现有 crate 依赖中；Rust toolchain 为 1.93.1、workspace edition 为 2024。新增文件使用 Cargo 默认的独立 integration test target `edited_fifo_recovery`，不需要改 Cargo.toml，也不改 library 源码输入。

实际执行过 `rustfmt --check --edition 2024 crates/codegen/chat-state/tests/edited_fifo_recovery.rs`。首轮仅报告 import/链式访问格式差异，已在本文件内修正；最终检查无格式诊断。此检查仅解析/检查格式，不解析依赖类型、不执行 Timeline validation，不能作为编译成功证据。

另核对了 `git status`、HEAD、现有 `timeline.rs` diff，并对新增文件使用 `git diff --no-index --check /dev/null <path>` 检查空白诊断；该 no-index 命令因新增内容存在返回 1，无空白错误诊断。未把未跟踪文件的普通 `git diff --check` 空输出当作覆盖证据。

**未执行：** Cargo 编译、四个测试、旧红/新绿、已有 fold 回归、真实文件 durability、Shell runtime/行身份、真实编辑到冷恢复链、OpenSpec 完成/归档检查。本 change 的功能任务仍未勾选。

## 父后续接续

父确认独占 Cargo 窗口并重新核对当前文件后，首个定向入口可为：

```sh
cargo test --locked -p chat-state --test edited_fifo_recovery -- --test-threads=1
```

这只是待授权执行的命令，本轮未运行。先确认失败来自上表的查询顺序断言；若先出现编译错误或非法 fixture，先修新测试并重新取得有意义旧红，不能把该类错误当产品旧红。取得旧红后，只有父 resume 明确授权时才进入 `timeline.rs` 最小 query/fold 修复及相关 new green/fold 回归。

Shell 原行 runtime identity 为下一个独立任务。此准备和后续 canonical query 通过均不能替代真实存储同步、进程冷恢复、Shell identity 或 Session F2/milestone 验收。
