# 对抗性审查

审查覆盖 live notification、history replay、child 生命周期终态和测试 fixture：

- **Child 身份与隔离：** ImageRecoveryOperation 由 session id 命中 child view 后写入 child scrollback；测试同时断言 parent scrollback 没有冒领行。未找到 child view 时不投影。
- **Prompt 终态边界：** child live TurnCompleted 必须匹配当前活动 prompt；可选 envelope prompt id 与 payload 不一致时拒绝。replay 按历史顺序只收尾 payload prompt 对应的行。实现不调用 root turn finalizer，也不改变 child 会话 turn 生命周期。
- **其他 child 收尾：** SubagentFinished 使用 child finalizer 收尾该 child 的未完成图片恢复；有专属终态的行仍由 operation 状态控制，迟到 progress 不重新打开终态行。
- **Replay 顺序：** 初版独立读取 ACP 和 Grow history 会把 recovery 放到后续 ACP 输出之后。审查确认 eventId/timestamp 不能可靠恢复物理顺序，因此改为扩展同一个 reconciled `stream_session_history_at` reader 输出 ImageRecoveryOperation 和 TurnCompleted；磁盘回归包含 Started、ACP agent message、TurnCompleted 的有序 fixture。
- **Fixture 校正：** compaction 用例此前通过 child ACP Plan 初始化 TODO，但 child Plan 通知并不更新 child `AgentView.todo`；改为显式 seed child TODO，只验证 compaction 保留行为，并继续验证真实 root Plan 通知能覆盖 root TODO。history fixture 此前缺少 Timeline 文件，现写入空 Timeline，满足 reader 的 reconciliation 前置条件。
- **仍需主 agent 验证：** 上述 fixture 修复后的 Rust 回归尚未重跑；验证结果到达后应更新 `verification.md` 并据结果勾选 `tasks.md`。
