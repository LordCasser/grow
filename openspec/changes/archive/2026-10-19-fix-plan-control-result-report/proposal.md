## Why

Plan 完成后先退出 Normal，结果投影再把最终报告替换成已清除的 artifact 引用，导致显示 `artifact:plan:unavailable`。用户要求完整原始报告在顶部 Plan 详情的结果 Tab 中查看，主视图保留简短总结；已完成入口保留到下一次用户手动切换 Behavior。

## What Changes

- 完成工具接收原始完整 `report`；成功后恢复正常采样，在主视图生成简短总结。
- 完成提交将计划和报告的不可变引用保存在原有 Control 快照中，与退出 Normal 一起持久确认；canonical 查询返回两份内容。
- 顶部保留 `[Plan: Complete]`，详情提供 Plan / Result report Tab，键鼠均可切换；完成详情只读，TODO 独立保留。
- 下一次成功的手动 Behavior 选择（包括选择当前 Normal）清除已完成入口；拒绝、确认中、自动退出和恢复不能提前清除，迟到查询不能复活旧详情。
- 主视图显示简短总结，删除原有 artifact 占位覆盖。

## Capabilities

### New Capabilities
- None.

### Modified Capabilities
- `client-surfaces`: 完成 Plan 的顶部入口、双 Tab 详情、简短总结与清理边界。
- `behavior-goal`: 完成报告引用的持久控制状态及手动切换清理规则。

## Impact

影响 PlanControl 输入、Shell Control 快照与 canonical 查询、Pager Plan 详情及状态投影。复用现有不可变 artifact 存储、Control 持久屏障和 LineViewer；不引入新的状态 owner 或通用 Tab 框架。
