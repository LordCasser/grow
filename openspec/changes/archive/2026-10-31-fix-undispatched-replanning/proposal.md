## Why

子任务因 cwd 越界在启动前失败后，Planning 保留明确的 dispatch_failure 和非 terminal 关联。后续父 TODO 修改只检查 terminal，重新为该不可运行的 child 创建确认义务；Released 不解除这项义务，完成检查持续要求模型重试。

## What Changes

- 明确未派发的关联不再生成需要 child 确认的更新，保留来源及无法应用的原因。
- 恢复及投影前依据既有 dispatch_failure 闭合遗留 replan；重复记录同一失败也能完成闭合，不伪造执行终态或成功结果。
- 同一模型循环提示处理后，相同完成阻塞再次返回时停止本轮，沿既有错误结算路径暂停 Goal 内部续跑；普通 final 与 StructuredOutput 共用该规则。
- 补充先启动失败、后父项修改，以及遗留状态恢复、实际模型调用次数的回归。

## Capabilities

### Modified Capabilities

- `task-planning`：未派发关联的重规划闭合与无进展完成检查。

## Impact

仅修改共享 LocalPlan 转换、SessionActor 的计划恢复和完成检查及相关测试、开发者说明。不调整权限边界、执行 owner、预算或用户暂停接口，不将无通信路由本身视作执行已终止的证据。
