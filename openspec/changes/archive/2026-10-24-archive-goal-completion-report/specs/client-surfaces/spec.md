## ADDED Requirements

### Requirement: Done Goal details expose the archived completion report

Pager SHALL 通过现有状态栏 `Goal: Done` 点击入口，在 Goal 详情中显示归档的完整完成报告与原目标，并允许滚动查看长报告。报告 SHALL 在用量前展示。实时通知、恢复与只读回放 SHALL 保留报告原文，历史 Complete 没有报告时 SHALL 明确显示未归档，而不是推测完成原因。

#### Scenario: Open Done Goal
- **WHEN** 用户点击有完成报告的 `Goal: Done`
- **THEN** 打开既有 Goal 详情并可查看完整报告、原目标与用量，长报告可通过既有键盘和滚轮操作浏览。

#### Scenario: Restore completed Goal
- **WHEN** 已完成 Goal 被恢复或只读回放
- **THEN** 详情保留归档原文，不重新生成报告或发起 Goal 工作。

#### Scenario: Workflow history coexists with Done Goal
- **WHEN** 会话仍保留 Workflow 历史，用户点击当前 `Goal: Done`
- **THEN** 打开 Goal 报告详情，Workflow 历史不得截走该入口。

#### Scenario: Historical report is absent
- **WHEN** 用户查看没有已归档报告的历史 Complete Goal
- **THEN** 详情明确说明该 Goal 没有归档的完成报告。

实施入口：`shell/src/session/goal_notification.rs`、`shell/src/session/history/grow_facts.rs`、`pager/src/app/acp_handler/session_notification.rs`、`pager/src/views/goal_detail.rs`。
