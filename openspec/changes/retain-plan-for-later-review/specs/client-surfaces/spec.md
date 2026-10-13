## ADDED Requirements

### Requirement: Plan previews resolve the canonical reviewed artifact
/view-plan 和计划预览 SHALL 从后端候选引用解析不可变正文与版本，不依赖 legacy plan.md 或残留审批缓存决定当前内容。引用变化 SHALL 使旧审阅决定失效并提示重新审阅。

查询 SHALL 使用有界异步读取，不阻塞 Pager render/输入路径；返回值与评论 SHALL 绑定 session、当前查看请求和候选版本。

#### Scenario: Only the immutable artifact exists
- **WHEN** 审批 view 已不存在且没有 legacy plan.md，但候选 artifact 有效
- **THEN** 仍能查看正确版本，界面展示与后端审批依据一致。

#### Scenario: Preview returns after session or version changes
- **WHEN** v1 正文请求尚未返回时用户切换会话、关闭重开预览或当前候选替换为 v2
- **THEN** 旧返回不覆盖当前展示，v1 评论被保留并标记原版本，不自动套用到 v2 相同行号。

实施入口：`crates/codegen/pager/src/app/agent_view/plan.rs::plan_body_for_preview`、`slash/commands/view_plan.rs`。

### Requirement: Plan dismissal and defer expose different decisions
客户端 SHALL 区分收起、保留稍后审阅、批准并执行、要求修改和放弃，保留可发现入口、用户草稿及相关评论。收起仅改变展示；保留是否成功以后台接纳为准。界面 SHALL 保留 Grow 最顶部 task 布局，不依赖统一 Dock。

#### Scenario: Collapse and reopen review
- **WHEN** 用户收起 v1 后重新打开
- **THEN** 查看同一版本且未产生批准/拒绝/执行，输入草稿不丢失。

#### Scenario: Defer request is not acknowledged
- **WHEN** 保留请求的接纳结果未确认
- **THEN** 显示未确认并保留内容，不以本地关闭卡片宣称持久保留成功。

实施入口：Pager Plan view、原焦点/草稿 owner、Shell typed control response。

### Requirement: Plan execution feedback waits for authoritative admission
批准执行后客户端 SHALL 区分接纳处理中、已接纳、明确拒绝和状态未确认；在决定未获权威确认前保留候选与评论，不允许相互冲突的决定被乐观视为成功。

#### Scenario: Storage admission is closed
- **WHEN** 后端因准入或持久化故障未接受执行
- **THEN** 候选保持可审阅，展示受阻/未确认原因，不清空或再次作为普通 prompt 提交。

实施入口：Pager Plan action/response、现有 Shell approval/Control ACK。
