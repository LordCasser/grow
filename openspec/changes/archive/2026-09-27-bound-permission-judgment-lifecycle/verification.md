# Verification

## 契约核对

- 截图中的英文故障文字来自 `PermissionManager` 子 Agent Auto 模型裁决超时分支，不是 ACP 人工提示 deadline；主会话 `always-approve` 不覆盖子 Agent 创建时冻结的独立 `Auto` mode。
- 主/子会话共用分类通道；过期结果不会形成一次性 permit。主会话 Auto 分类超时仍退回人工提示，子 Agent Auto 裁决超时仅拒绝当前工具调用。
- `PermissionManager` 的人工提示由 `AcpPrompter::request_for_session` 限时；迟到响应的接收端已关闭，既有 `prompt_timeout_is_distinct_and_actor_accepts_next_request` 和 Pager teardown 回归覆盖该路径。

## 已执行

- `cargo test --locked -p workspace --lib classify_channel_carries_request_enqueue_time -- --test-threads=1`：1 passed。
- `cargo test --locked -p shell --lib permission_judgment -- --test-threads=1`：4 passed，覆盖冻结主任务上下文、Sideband admission 卡住的总期限与迟到确认、首次尝试预算。
- `cargo test --locked -p shell --lib child_judge -- --test-threads=1`：2 passed，覆盖真实主上下文判断和无效/瞬态响应的有限重试。
- `cargo test --locked -p workspace --lib child_auto_judgment_timeout_fails_closed_without_prompt -- --test-threads=1`：1 passed。
- `cargo test --locked -p workspace --lib prompt_timeout_is_distinct_and_actor_accepts_next_request -- --test-threads=1`：1 passed。
- `cargo test --locked -p workspace --lib auto_classifier_timeout_preserves_total_denial_limit -- --test-threads=1`：1 passed，主会话 Auto 分类超时路径。
- `cargo test --locked -p shell --lib subagent_bash_permission_timeout_does_not_execute -- --test-threads=1`：1 passed。
- `openspec validate bound-permission-judgment-lifecycle --strict --no-interactive`：通过。
- `openspec validate --all --strict --no-interactive`：16 passed。
- `git diff --check`：通过。

## 归档核对

- 归档前 `openspec validate --all --strict --no-interactive`：16 passed。
- `openspec archive bound-permission-judgment-lifecycle --yes`：成功，将新增要求合入 `tool-authorization`。
- 归档后 `openspec validate --all --strict --no-interactive`：15 passed。
- `openspec validate --archived --no-interactive`：530 passed。
- `rustfmt --edition 2024 --check`（四个修改的 Rust 文件）：通过。
