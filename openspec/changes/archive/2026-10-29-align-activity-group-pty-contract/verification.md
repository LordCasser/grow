## Verification

- `openspec validate align-activity-group-pty-contract --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: 19 passed, 0 failed.
- `rustfmt --edition 2024 crates/codegen/pager/tests/pty_e2e/verb_group_fold_expand_collapse_pty.rs`: passed.
- Scoped `git diff --check`: passed.
- First real PTY execution after aligning the summary failed at the close-body check (`/tmp/grow-2.5.3-activity-pty.log`). The test observed the expanded group before the PTY processed the close-body redraw. Synchronization was revised to wait for the body sentinel to disappear, and collapse now waits for member-row disappearance instead of sleeping.
- The root validation workflow passed the target ignored PTY test: 1 passed in 8.03s (`/tmp/grow-2.5.3-activity-pty-final.log`). Root then shared one absolute 90-second deadline between waiting for the completion sentinel and turn-idle state, so those phases do not each receive a fresh 90-second allowance.
- Root repeat execution and the remaining selected PTY matrix were still in progress when this record was updated (`/tmp/grow-2.5.3-activity-pty-repeat.log`, `/tmp/grow-2.5.3-remaining-pty.log`). Cargo was run by the root validation workflow, not by this task.

最终活动组PTY又精确运行3次，全部通过，日志 `/tmp/grow-2.5.3-activity-pty-repeat.log`。此前未跑到的16项正式发布PTY各自精确枚举并执行1项，全部通过，汇总 `/tmp/grow-2.5.3-remaining-pty.log`，独立日志 `remaining-pty-01..16.log`。包含真实复制/滚动、队列取消、多客户端、三层树挂起冷恢复、显式技能快照、Plan恢复、部分叶释放与并发冷恢复唯一root，未增期限或跳过原场景。
