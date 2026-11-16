# 验证与验收

2026-11-17，固定Rust1.93.1。架构owner亲读layout diff与调用方，确认gap_rows只描述真实布局height..height+gap_after内的偏移，正文仍按逻辑行和sub-row夹紧。估算布局转精测时只恢复gap pin，不改变普通正文的settle策略。

`cargo test --locked --lib -p chat-state -p shell -p pager -p pager-minimal -p tool-types -- --test-threads=4`中的Pager整库结果为7719 passed、0 failed、15 ignored。新增三项全部通过：

- gap_anchor_roundtrips_in_same_geometry：一行Tool加真实tail使offset1可达；capture/restore后仍为1且下一entry在屏幕第1行。
- gap_anchor_follows_entry_after_history_prepend：既测原prepend_entries_from，也测连续窗口实际采用的replace_history_prefix；按同EntryId末尾保位。
- gap_anchor_follows_rewrapping_entry_end：真实wrapping高度在80→30、30→120变化后，仍锚在原entry末尾间隙。

原resize_clamps_subrow_within_rewrapping_anchor_line与follow_without_prompt_preserves_a_gap_row均通过；未删除或放宽正文锚点断言。单条entry总高度等于viewport时offset1不可达，夹具补tail，不改变bottom clamp。Minimal104项也通过。

后续remove-unused-history-page-paths维护删除了无生产caller的旧prepend入口；当前gap前插回归仅走主历史窗口实际使用的replace_history_prefix，并保留所有该路径身份和行位置断言，固定1.93.1重跑通过。原双路径执行结果作为当时的历史验证保留。

`cargo check --locked --workspace --all-targets`、固定1.93.1的`cargo fmt --all -- --check`、`git diff --check`均退出0。没有改历史cut、用量、native提交事实或公共/持久化schema。docs/development同步布局责任和主规范链接；本轮发现已闭合，可归档。
