## Why
恢复先截取最新 128 个 Workflow Spawned，再验证文件。较新的 cleared、缺失或损坏记录会占用名额，使较旧的有效 run 丢失。

## What Changes
- 名额改为最多恢复最新 128 个有效 run，验证失败的候选不占名额，最终仍按 Timeline 顺序返回。
- 保留 lifecycle、脚本/参数 hash、有界读取和错误处理规则。
- 不处理 Forgotten 事件或进度 checkpoint，它们涉及独立持久化语义。

## Capabilities
### New Capabilities
无。
### Modified Capabilities
- workflow-execution: 有效恢复项上限。

## Impact
shell JSONL Workflow 恢复及测试。最坏情况下扫描全部有限 Timeline 候选，这是回补有效 run 的必要成本。

