## Why

session `01a08910-219a-78f1-a45c-98b448764a05` 在异步压缩后只输出准备执行动作的一句话便结束。原始响应有合法 `stop` 和 `[DONE]`，并非流截断；压缩把已完成旧任务的摘要带回正在执行新任务的上下文，却没有给后续 Step 明确的任务续接边界。

## What Changes

- 压缩摘要明确只描述被替换的历史；后续保留消息代表更新的请求和进度，摘要中的完成/等待结论不能覆盖它们。
- 前台普通 turn 在边界发布异步压缩结果，并确实获准开始下一 Step 时，在保留 tail 后持久化一个 `AutoContinue` 提示。提示继续最新授权任务，不授权新工作、不要求额外重试。
- 已结束、取消、控制终止、预算终止、仅后台生成/失败或手动压缩不因该提示打开新回合。
- 保留 native epoch 清理及合法 `stop` 语义，记录工具历史扁平化的独立架构债务。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `context-compaction`：摘要的时间范围，以及异步发布到下一 Step 的任务续接提示。

## Impact

仅涉及 Shell 的摘要组装、between-step 准入和对应集成测试、开发者说明。复用 Timeline、`SyntheticReason::AutoContinue` 和既有控制 gate；不新增 actor、持久化格式、依赖、配置、模型调用或完成判定框架。不修改正在实施的 `unify-sampling-attempt-recovery` 的协议恢复、结算或预算策略。
