## ADDED Requirements

### Requirement: Completed Plan details separate the full report from the summary

Plan 完成后，客户端 SHALL 保留顶部已完成状态入口，详情 SHALL 以 Plan 和 Result report 两个 Tab 分别展示原计划与原始完整报告，并支持键鼠切换。完成详情 SHALL 为只读，不能复用审批或评论操作；TODO 入口与状态独立保留。主视图 SHALL 显示正常的简短总结，不以完整报告或 artifact 占位引用替代。下一次成功的手动 Behavior 选择 SHALL 清除已完成入口及缓存；完成自动退出、恢复、拒绝或待确认不能提前清除。已失效的查询响应 SHALL NOT 复活旧详情。

#### Scenario: Complete and inspect the report
- **WHEN** Plan 已完成并持久退出 Normal
- **THEN** 主视图显示 summary，顶部显示已完成 Plan；打开详情可在原计划与原始完整报告间切换，TODO 不被清空，报告页不能审批或发送评论。

#### Scenario: Restore a completed Plan
- **WHEN** 完成后重新连接或恢复 session，且用户未再次手动选择 Behavior
- **THEN** 完成入口仍可发现，通过 canonical 查询可恢复两份内容，不触发模型工作或重做执行。

#### Scenario: Clear on the next successful manual selection
- **WHEN** 用户随后成功手动选择任意 Behavior，包括当前 Normal
- **THEN** 清除完成状态栏、完成详情与对应缓存；迟到的旧查询不能重新打开。

#### Scenario: Selection is not accepted
- **WHEN** Behavior 选择被拒绝、仍待确认，或仅收到完成后的自动退出更新
- **THEN** 已完成入口及报告保持可查看。

#### Scenario: Other Plan control results
- **WHEN** 提交、修订或取消成功，或者完成输入非法
- **THEN** 显示真实审批、取消或错误消息；失败不修改原 Plan，不生成 unavailable 占位。
