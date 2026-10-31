## Context

RPC 与 durable terminal 两路都把 cancelled 归入 `SessionEvent::TurnCancelled`；该事件只有耗时，没有来源。`TurnOutcome::Cancelled` 也用于 Step/foreground/provider 准入关闭，因此旧 renderer 的“by user”没有事实依据。`AutoCompactCompleted` 本身只更新压缩展示和计数。

## Goals / Non-Goals

目标是纠正已证实的错误归因，并检验反馈涉及的压缩续跑路径。非目标是扩充终态协议、重构准入或按截图猜测取消根因。

## Decisions

沿用共用 renderer，只移除“by user”；真实用户取消也采用同一中性文案。更新现有文本断言，保留事件身份和两路终态去重。Plan 回归使用已批准的执行状态及现有 HTTP mock，断言摘要已提交、后继主请求执行、同一 Turn 正常完成；既有失败/取消测试验证不会因此放行非法续跑。

## Risks / Trade-offs

展示不再指出操作者；已有事件本来就无法支持这一归因。没有现场证据，测试通过只能排除覆盖的场景，不能证明原反馈根因消失。工作树中其他 Plan 修改由其原任务持有，本次不修改这些实现。
