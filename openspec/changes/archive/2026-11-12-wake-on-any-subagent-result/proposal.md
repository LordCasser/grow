## Why

同时等待多个 subagent 时，当前 join_all 与 wait-all 提示词会让先完成的结果一直等到其他任务完成或预算耗尽，阻止主 agent 及时分析结果并推进工作。用户明确要求任意一个完成即结束本次等待。

## What Changes

- wait_subagent 对多个目标采用 wait-any，返回 any_finished、完成者 ID、真实终态及结果。
- 保留每个目标的身份和最近实际观察；因其他目标完成而停止的未观测项明确 observation_interrupted，不伪造运行或完成状态。
- 保留单一总期限、失败/取消终态、挂起/收尾非终态、用户中断与完成通知的现有所有权。
- 同步动态提示词与开发者说明，要求主 agent 先分析返回结果，仅对剩余依赖继续等待。
- 将本次对抗分析发现的独立债务记录在 audit.md 与 backlog，不混入本次实现。

## Impact

修改 Tools 等待聚合、公共结束原因及 Agent 动态提示词；不增加工具、等待模式开关或兼容分支。
