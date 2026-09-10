## Why

强制 FinishTurn 把合法 provider 结束误判为缺失声明，需要额外采样；声明本身又不证明任务完成。当前中性 ToolCalls 会覆盖拒绝/截断，TurnTerminal 的本地 end_turn 标签也不能独立说明 provider 来源。用户要求尊重三个 API 的原生结束，区分宿主补全，补足方案盲区后实现。

## What Changes

- 移除普通 Turn 的 FinishTurn 强制工具、恢复提示及连续违约机制；合法有正文且无待执行调用的正常 provider 结束恢复正常收尾，标点不参与判断。
- 单独保留 provider 原始终止事实与中性终止语义，调用是否存在不再覆写原因。拒绝不会因调用完整而进入普通完成或业务执行。
- 复用现有 request/attempt evidence 与 Turn::Ended；成功请求记录对应 provider terminal，Turn source 关联该 request。宿主控制、错误、用户取消和崩溃恢复记录其来源，不伪造 provider terminal。
- 保留显式 Goal、Stop hook、结构化输出以及截断/压缩恢复的既有调度职责，同步相关规范与开发者说明。
- 同步独立进程协调 mock：通过现有协调能力识别普通前台请求，删除完成工具的特殊分支。

## Impact

sampling-types、sampler、ChatState、Shell 的 Turn/event tracker、相关测试与规范。原始会话只读，不回写历史来源；历史未记录来源只能显示未记录，不能推断为 provider。目标 ID 映射与 Responses phase 的完整 portable 契约保持独立 backlog，不混入本次完成来源修复。
