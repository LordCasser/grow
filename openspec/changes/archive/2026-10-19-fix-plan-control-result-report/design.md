## Context

`execute_tool_calls_batch` 在 Complete/Cancel 成功时先持久退出，清空当前 Plan runtime；结果发送层再用当前引用覆盖工具消息，既遮蔽报告又生成 unavailable 占位。现有 canonical 查询只解析活跃/待审阅 artifact，单文档 PlanPreview 无法展示完成报告。

## Decisions

- PlanControl Complete 必须提供原始 `report`。报告原文写入已有内容寻址 artifact 存储，工具结果仅确认成功；完成控制后在 Normal 下重新采样，按工具说明生成简短正常回复，不由宿主伪造 assistant 消息。
- 现有 BehaviorCoordinator/Control 快照增加独立的 completed Plan 引用（计划 submission/revision/hash 与报告 hash），不把完成状态伪装成活跃 Plan。写入报告后，将已完成引用与退出 Normal 一次持久提交；失败恢复原状态并禁止发布完成结果。
- canonical 只读查询在完成后读取计划与报告；查询身份核对同时覆盖完成引用。恢复仍显示完成入口，fork 不继承父 session 的完成展示身份。
- CurrentModeUpdate 发布完成标记。Pager 只按权威投影显示 `[Plan: Complete]`，完成详情在既有 PlanPreview 中提供两个 Tab，报告页只读并禁用评论/审批操作；仍保留 TODO 独立入口。
- 下一次手动 Behavior 选择成功后，在同一控制提交中清除完成展示；同模式选择也适用，失败或仅要求确认不清除。自动退出与恢复保持完成引用；清除后丢弃完成缓存及未返回查询，避免迟到结果复活。
- 删除结果发送层的无条件覆盖和失去调用方的引用格式化方法。

## Validation

后端覆盖报告原文、简短总结、完成引用恢复/读取、写入及提交失败、取消和非法完成、手动同/异 Behavior 清除及拒绝保留；Pager 覆盖顶部状态、双 Tab 键鼠、只读限制、TODO 独立与迟到查询失效。通过隔离构建排除工作区无关未完成改动。
