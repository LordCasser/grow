## Why

旧审查子任务恢复到Receipt阶段仍无法发布到父计划。公共API实验确认：父合法以resultId=null释放交付之后，LocalPlan要求全部历史review的resultId始终等于link最新resultId，迟到canonical结果因此永远不能提交，无法记录真实终态或处置未应用replan。

## What Changes

- 修正planning状态契约：历史Released(None)保留为“结果到达前已释放”的不可变事实，不要求绑定后来结果；Released(Some(ref))与Accepted/NeedsRevision仍必须绑定真实当前结果。
- late canonical结果按原入口验证并提交link终态、replan未应用状态；释放不创造result/terminal或自动接受结果。
- 同一状态owner的回归覆盖提前release、迟到结果、控制与非法review反例；记录宿主采用新构建之前的现场阻塞限制。

## Capabilities

### Modified Capabilities

- task-planning：显式释放可以与迟到canonical结果共存，保留执行/交付/验收分离。

## Impact

写面限common/tool-types的LocalPlan review校验与相关测试；shell仍验证canonical引用并提交计划。无新公共实体、操作权限、工具action或手写运行态修复。需要构建包含修正的Grow并通过宿主受支持恢复路径处理当前现场；本地源码修正不能冒充运行进程已更新。
