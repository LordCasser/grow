# Design

`LocalPlan::validate_transition_from` 是 scope transition 的既有规则权威。actor 在收到 `SelectScope` 时构造候选空计划，并以候选计划验证当前计划的 transition；只有校验通过才替换计划并提交。这样复用已有规则：显式 dispatch failure 可继续切换，但 required delivery 仍需 Released；活跃或结果未知 link、wait、未处理 replan 及其他未闭合义务继续阻止切换。

不增加状态或接口，不改变 link 的 terminal/result 事实，不修改共享 tool-types。

## Validation

新增 actor 命令级测试覆盖可切换和应阻止的矩阵，并断言失败时 timeline 计划不变。由主代理运行定向验证。
