## 验证结果

- `cargo check --locked --offline -p shell -p pager` 通过。`rustfmt --edition 2024 --check` 覆盖修改的 Rust 文件，`git diff --check` 通过。
- Shell 定向测试通过：`queue_control_hold_save_remove_checks_version_and_edit_identity` 覆盖版本、编辑身份、多客户端冲突、放弃编辑原文不变、持久替换和撤回；`rejected_replacement_keeps_original_input_and_edit_hold` 覆盖 admission 失败；`disconnected_client_releases_only_its_queue_holds` 覆盖断线只释放所属 hold；`queue_hold_and_promotion_share_one_control_boundary` 覆盖提升／hold 竞争，`held_queue_head_stays_pending_ahead_of_goal_continuation` 覆盖 Goal Active 下 held 队首阻止续跑。`queue_control_ext_request_returns_actor_acknowledgement` 及 Leader 的注入／断线测试覆盖 ACP 回执和连接身份。
- `cargo test --locked --offline -p prompt-queue --lib skips_row_under_edit -- --nocapture` 通过：较早的无 hold 前缀可合并，合并在 held 项前停止。
- Pager 定向测试通过：`goal_active_queue_keys_edit_and_remove_pending_row_without_stopping_goal`、`goal_active_queue_edit_click_focuses_queue_before_hold_ack` 覆盖 Goal Active 的键盘、鼠标操作及当前 turn 保持；`shared_queue_edit_waits_for_hold_and_preserves_text_on_failed_save` 覆盖 Normal 下的回执前不入编辑态和保存失败保稿；`optimistic_shared_queue_echo_cannot_claim_an_edit_hold` 覆盖未确认回显；`rebinding_a_view_releases_its_old_session_edit_hold` 与 `late_queue_hold_ack_releases_the_confirmed_server_hold` 覆盖重绑、迟到和丢失回执的清理。
- 真实 Pager/ACP/PTY：先执行 `cargo build --locked --offline -p cli --bin grow`，再以 `PAGER_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow cargo test --locked --offline -p pager --test pty_e2e_queue <test> -- --ignored --nocapture` 分别运行 `verify_bashq_claim3_edit_keeps_bash` 与 `removed_queued_prompt_never_sent`，各 1/1 通过。前者确认正在执行的 turn 结束后只运行编辑后的 bash 内容、原内容未发送给模型；后者确认撤回内容未进入任何后续模型请求、队列中的另一项仍按 FIFO 提升。两个旧 opt-in 场景此前缺 mock LLM 配置，撤回场景还会落入项目选择弹窗；本 change 补齐测试夹具，并在发送 Enter 前等待输入回显、Enter 后等待队列行，以免把未接纳的输入误判为已撤回。
- `openspec validate hold-and-manage-goal-queued-input --strict --no-interactive` 通过。
- 归档后 `openspec validate --all --strict --no-interactive` 15/15、`openspec validate --archived --no-interactive` 429/429 通过；`git diff --check` 通过。

## 边界

hold 是进程内编辑租约，不写入 Timeline；保存／撤回的输入接纳与 dismiss 是 durable 的。普通本地 ACP 连接结束会销毁会话 actor；Leader 多客户端断线通过内部通知仅释放该连接的临时 hold。`grow/queue/changed` 用于投影，命令结果以 `grow/queue/control` 的回执为准。回执丢失时 Pager 不宣称成功，尽力发送幂等 release，并保留未确认的编辑文本。
