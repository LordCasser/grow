## ADDED Requirements

### Requirement: Unconfirmed drafts retain their original submission identity

Pager 对未确认发送恢复的草稿 SHALL 保留原 session/prompt identity、请求摘要、可用的不可变快照及接纳未知 disposition，并遵守既有草稿单源与大小限制。自动恢复不得生成新身份派发。原请求重试 SHALL 核对原输入且使用精确原快照；快照不可得时只能核对状态或显式新发送。编辑内容的新发送必须是显式新意图。迟到 ACK 不得覆盖新绑定或删除用户的新编辑内容。

#### Scenario: Draft is restored after an ambiguous send
- **WHEN** 发送响应丢失，客户端恢复文字和图片
- **THEN** 显示接纳尚未确认并关联原提交，不声称服务端未收到，不自动新建请求或重复执行。

#### Scenario: Original submission was already admitted
- **WHEN** 核对发现原输入已排队、执行或完成
- **THEN** 草稿恢复项关联原输入状态，不再派发另一输入，用户后续编辑保持可用。

#### Scenario: User edits an uncertain draft
- **WHEN** 用户修改尚未确认的原草稿并显式发起新发送
- **THEN** 展示旧提交的已知/未知状态，为新意图使用新身份；不会将修改后的内容重放到原身份。

#### Scenario: Local checkpoint fails before process exit
- **WHEN** 原提交身份或草稿内容未能保存到本地存储
- **THEN** 页面保留当前内存内容并显示未保存限制，不声称退出后必然能够恢复。

#### Scenario: Original request snapshot is unavailable after editing
- **WHEN** 草稿已保存新编辑内容，旧提交仅余 identity/摘要而精确快照不可得
- **THEN** 页面保留旧提交核对入口及新内容，不把新内容绑定旧 identity 重试，也不保存第二份冲突的草稿来源。

证据与实施入口：`crates/codegen/pager/src/app/root/dispatch/prompt.rs`、`crates/codegen/pager/src/app/session/mod.rs::InFlightPrompt` 及既有 local draft 路径。

### Requirement: Recovery resource protection exposes actionable limitations

因恢复依赖或证据不足跳过清理时，系统 SHALL 显示资源归属和具体保留原因，不报告删除成功。收尾状态 SHALL 区分已持久保全、仅内存暂存和无法保全；不得承诺在不可写存储与进程退出同时发生时恢复所有未提交内容。诊断与保留项列表 SHALL 有界。

#### Scenario: Automatic maintenance preserves an unresolved worktree
- **WHEN** 清理因未确认结果、未完成 snapshot 或引用无法校验而跳过 worktree
- **THEN** 用户可识别关联任务及保留原因，清理统计不把它算作已删除。

#### Scenario: Volatile result retention exceeds its allowance
- **WHEN** 未提交正文无法在内存预算内继续保留
- **THEN** 显示正文保全限制及已有持久证据的位置，不伪造内容已保存或任务仍在执行。

证据与实施入口：`crates/codegen/shell/src/session/storage/jsonl/mod.rs::cleanup_stale_sessions_sync`、`crates/codegen/workspace/src/worktree/mod.rs`、第一项新增的运行健康展示。
