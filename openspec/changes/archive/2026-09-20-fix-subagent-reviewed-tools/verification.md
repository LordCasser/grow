# 验证记录

## 修复结果

本 change 沿 `finalized ToolBridge → child capability → PermissionManager → one-shot permit → dispatch` 建立一个闭环：工具 descriptor 可以声明 `subagent_review=required`；`write` 因此在最终过滤后对 child 可发现、可提交精确调用，但不进入 initial-RWX fast path。`search_replace` 保持原有 authored/RWX 行为，hard-ineligible identity 仍在权限请求前拒绝。

权限请求从 Shell 已冻结的 wire call 生成 `PermissionCallEvidence`，保留真实 tool name、canonical args hash 和有界的 whole-file replacement 摘要。PermissionManager 的 classifier、主上下文判断和 audit 在有 evidence 时使用真实 identity；非 Shell/旧内部调用没有 evidence 时仍使用既有 `AccessKind` 回退。允许结果只签发当前调用的一次性 permit，不扩张 child capability，也不把 `ask_parent` 回复解释成授权。

| 场景 | 动态证据 |
| --- | --- |
| descriptor 默认行为、`write` 的 Required 声明及最终 bridge 投影 | tool-protocol `subagent_review_defaults_to_inherited_behavior`；tools `finalized_write_retains_required_child_review_policy` |
| ReadWrite/All 下 `write` 必审，普通 `search_replace` 直通 | shell capability `descriptor_review_admits_but_never_fast_paths_a_native_tool`；actor `injected_write_is_reviewed_per_call_while_matching_edit_keeps_fast_path` |
| 最终 Agent 过滤仍可移除 runtime write | agent `final_agent_filter_removes_runtime_write_before_capability_projection` |
| Auto allow/deny 与 AlwaysApprove/Ask 既有语义 | actor `reviewed_write_auto_honors_allow_and_deny_without_human_prompt`、`reviewed_write_always_approve_keeps_explicit_operator_semantics` 及逐次 Ask 回归 |
| visible forbidden 不可申请，并指向父级协调而非会话授权 | shell `forbidden_catalog_explains_that_parent_coordination_is_not_approval`、`subagent_hard_forbidden_bash_rejects_before_permission` |
| exact identity、长内容有界摘要及 legacy fallback | shell `write_permission_evidence_is_exact_canonical_and_bounded`；workspace `frozen_call_identity_drives_child_classifier_and_audit`、`missing_frozen_call_evidence_keeps_access_kind_name_fallback`、`frozen_call_evidence_is_explicit_untrusted_classifier_input` |
| deny 优先、permit 单次消费、参数/authority/epoch 改变后失效 | workspace `policy_deny_and_auto_allow_record_no_decisions`；shell `permit_is_consumed_exactly_once`、`frozen_arguments_and_authority_are_revalidated`、`child_authorization_epoch_change_invalidates_permit` |

## 命令结果

- 受影响 Rust 文件的 `rustfmt --edition 2024 --check ...`：通过。
- `cargo check -p tool-protocol -p tools -p agent -p workspace -p shell`：通过。
- 上表所列定向测试：全部通过。Shell actor 测试使用 `RUST_MIN_STACK=16777216`；默认测试线程栈的一次运行发生 stack overflow，增加测试栈后相同回归通过。macOS 链接保留既有大型 debug 二进制 `__eh_frame` warning。
- `git diff --check`：归档前通过。
- `openspec validate --all --strict --no-interactive`：归档前 20 passed / 0 failed。
- 已归档为 `2026-09-20-fix-subagent-reviewed-tools`，delta 已合入 `tool-authorization` 主规范；归档后 `openspec validate --all --strict --no-interactive` 为 19 passed / 0 failed，`openspec validate --archived --strict --no-interactive` 为 348 passed / 0 failed，最终 `git diff --check` 通过。

严格 Clippy 没有宣称全绿：`cargo clippy ... -- -D warnings` 先被不在本 change 的 `token-estimation`、`auth`、`workspace-types` 既有 lint 阻塞；加 `--no-deps` 后仍被 tools 其他模块的既有 lint 阻塞。本 change 没有顺带修改这些债务。

完整 Shell integration target 也没有宣称通过：`cargo test -p shell --test test_grow_session_update --no-run` 被工作树另一项尚未完成的变更阻塞，`test_grow_session_update.rs` 尚未覆盖新增的 `SessionUpdate::ResponseReplayProjection(_)`。相关 Shell `--lib` 回归、受影响 crate check 均已独立通过，本 change 未修改该外部任务。

## 范围边界

没有新增持久化 grant、可变 session capability 或第二套主子通信协议；没有改变 primary Agent 的 `write` 行为。没有修复 `search_replace`/`write` 的跨 child、外部编辑器或跨进程 read→write 竞争，也没有进行真实 provider 审批、会话重启或文件竞态故障注入。未提交 Git。
