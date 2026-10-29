# Verification record

日期：2026-10-26

本记录将各场景对应到实际测试，并保留完整套件未全绿的验证限制。

## 场景到测试的映射

测试名均对应 Pager 源码中的真实测试函数。一个测试可覆盖多个场景；映射只说明测试实际断言的范围。

### Continuous tool and Thought activity forms a stable group

| 场景 | 覆盖测试 |
| --- | --- |
| Interleaved tools and Thought remain together | `mixed_tool_activity_keeps_leading_interleaved_and_trailing_thoughts`（`scrollback/state/groups.rs`）；`verb_group_streaming_thinking_keeps_one_run_then_folds`（`scrollback/state/selection.rs`） |
| Leading or trailing Thought retains the activity identity | `verb_group_leading_thought_anchors_run_and_expands`；`leading_thought_keeps_activity_header_when_thinking_is_hidden`；`set_show_thinking_blocks_flip_preserves_activity_identity_and_expansion` |
| Conversation, Skill, or standalone interaction separates activity | `skill_rows_and_conversation_entries_separate_activity_groups` 覆盖 Skill 行和新对话提示；`standalone_receipt_separates_activity_without_hiding_the_interaction` 直接断言独立通信回执隔开两段活动且自身可见。 |
| Thought-only activity has no tool group | `single_tool_with_thought_forms_a_group_but_pure_thought_does_not` |
| Group summary counts the whole group | `unified_activity_header_aggregates_tool_kinds_in_one_label`；`windowed_paint_renders_full_verb_group_label_for_offscreen_members`；`expanded_activity_header_renders_whole_group_label` |
| Failed tool remains inspectable | `mixed_tool_activity_keeps_leading_interleaved_and_trailing_thoughts`（展开失败成员仍有正文高度）；`unified_activity_live_and_batched_replay_have_the_same_group_and_summary`（失败成员仍在 live/replay 同一组，摘要报告失败数） |

### Activity group folding preserves active and manually revealed content

| 场景 | 覆盖测试 |
| --- | --- |
| Folded group retains active and explicitly revealed items | `folded_activity_keeps_running_explicit_pending_and_hook_members_visible`；`verb_group_refolds_on_pending_input_transitions`；`verb_group_unfolds_on_attach_hooks`；`unified_activity_preserves_manual_bodies_and_group_across_live_updates_and_resize` |
| Expanded group exposes ordered members and individual bodies | `expanded_activity_restores_native_member_heights_and_keeps_anchor`；`opened_multiline_first_member_remains_selectable_and_copyable`；`opened_first_thought_body_keeps_selection_below_the_summary` |
| Thinking blocks remain disabled or enabled as configured | `set_show_thinking_blocks_flip_preserves_activity_identity_and_expansion`；`leading_thought_keeps_activity_header_when_thinking_is_hidden` |
| Dense layout truncation encounters a grouped activity | `grouped_activity_is_not_truncated_again_by_dense_budget`；`windowed_paint_labels_activity_header_on_last_viewport_row` |

### Live activity updates and replay preserve grouping and user expansion

| 场景 | 覆盖测试 |
| --- | --- |
| Live append and completion retain an expanded group | `unified_activity_preserves_manual_bodies_and_group_across_live_updates_and_resize`（覆盖追加、成员完成、展开状态、手动打开成员和 resize） |
| Tool identity is refined after its description arrives | `unified_activity_preserves_manual_bodies_and_group_across_live_updates_and_resize`（将首成员从 Execute 细化为结构化 Read 身份，并断言组首 ID、范围、顺序和展开状态） |
| Layout resize and replay agree with live presentation | `unified_activity_preserves_manual_bodies_and_group_across_live_updates_and_resize`；`unified_activity_live_and_batched_replay_have_the_same_group_and_summary` |

Mouse routing 的专项回归是 `activity_header_hit_requires_the_header_row_to_be_visible` 与 `folded_running_activity_preview_keeps_header_and_member_click_routes_distinct`（`app/agent_view/selection.rs`）：顶部裁掉摘要行时不命中组头；折叠组首成员仍有预览时，双击可见摘要展开组，双击预览正文只操作成员。Replay 的对应 hit target 覆盖为 `expanded_group_header_and_first_member_have_distinct_mouse_targets` 与 `folded_activity_first_body_opens_details_even_with_its_summary_clipped`（`replay_cmd/browser.rs`）：已展开及仍折叠的活动组均区分摘要和首成员正文，顶部裁剪后正文点击不误操作整组。

## 验证结果

- Rust/Cargo 按 `rust-toolchain.toml` 显式使用 `~/.cargo/bin` 下的 Rust 1.93.1。
- `cargo fmt --all -- --check`：通过。
- `git diff --check`：通过。
- `openspec validate --all --strict --no-interactive`：17 passed，0 failed（记录于 `/tmp/grow-unify-openspec-before-archive.log`）。
- Archive 基线校验：710 passed，0 failed（`/tmp/grow-unify-archive-baseline.log`）。
- `cargo check --locked -p cli --bin grow`：通过；补齐 replay 路由后的最终检查也通过（`/tmp/grow-unify-final-cli-check.log`，44.43s）。
- Pager core：7598 passed，0 failed，16 ignored，12 filtered out（`/tmp/grow-unify-pager-core-2.log`）。该次命令显式跳过 `acp::spawn::tests::mcp_hot_reload`。
- 完整 Pager suite 的一次运行结果为 7592 passed、18 failed、16 ignored（`/tmp/grow-unify-pager-full-2.log`）。其中 7 项与本 change 有关的失败已在后续 core-2 验证中修复并通过；另 11 个 MCP 用例都在 UI 路径之前因 ACP initialize response timeout 失败。MCP 实现未在本 change 中修改；没有证据证明这些超时在基线也会发生。完整 suite 尚未全绿。
- Pager / Minimal 联合回归：Pager 7598 passed、0 failed、16 ignored、12 filtered；Minimal 95 passed、0 failed（`/tmp/grow-unify-final-tests.log`）。命令为 `RUST_MIN_STACK=16777216 cargo test --locked -p pager -p pager-minimal --lib -- --test-threads=4 --skip acp::spawn::tests::mcp_hot_reload`。
- 补齐独立交互边界与 replay 点击回归后的最终 Pager 测试：7600 passed、0 failed、16 ignored、12 filtered（`/tmp/grow-unify-final-pager-tests.log`），同样显式跳过 MCP hot-reload 测试族。新增的两项回归均通过。
- `cargo check --locked -p pager --benches`：通过（`/tmp/grow-unify-benches-check.log`），检查公开布局字段的 benchmark 调用方。

## 对抗审查及修复记录

- 已修复顶部裁剪导致首成员正文被误判为组头点击的问题，并增加可见摘要、被裁摘要及活动预览成员路由回归。
- 已区分成员原生高度与组投影高度，并修复含正文的组首槽未进入首次精确测量、从而裁掉正文的问题；相关的首成员正文选择/复制、重复投影与 resize 场景由上表所列测试覆盖。
- 已修复增量加入非 `Collapsed` Thought 时没有触发组结构重算的问题；`unified_activity_preserves_manual_bodies_and_group_across_live_updates_and_resize` 覆盖活动组增量追加及正文保持。
- 已修复默认 `respect_manual_folds = false` 时完成或工具细化处理覆盖用户手动展开正文的问题；组展开、正文状态与更新/resize 保持由 `unified_activity_preserves_manual_bodies_and_group_across_live_updates_and_resize` 覆盖。

## 归档及最终核对

- `openspec archive unify-tool-thought-aggregation --yes`：完成，向 client-surfaces 合入 3 项 Requirement。
- 归档后 `openspec validate --all --strict --no-interactive`：16 passed，0 failed。
- 归档后 `openspec validate --archived --no-interactive`：711 passed，0 failed。
- 最终 `cargo fmt --all -- --check` 与 `git diff --check`：通过。
- 清理了本次运行更新的 Pager / Minimal 增量编译缓存，释放 4.45 GiB；没有清理其他包或依赖缓存。
