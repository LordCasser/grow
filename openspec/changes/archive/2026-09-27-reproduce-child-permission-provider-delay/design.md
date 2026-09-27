## Context

子 Agent 默认 Auto 且不继承主会话 live mode。锁定调用经 PermissionManager 进入主 SessionActor 的分类通道，由独立 Sideband 发出 provider 请求。既有测试分别覆盖 Sideband 准备卡住、有效即时响应和无效响应重试，缺少 provider 已收到请求但延迟回应的完整路径。

## Decision

复用 `MockInferenceServer` 的精确 auxiliary 响应屏障和真实权限 actor。测试确认 provider 收到请求后推进虚拟时钟，再释放或保持屏障，检查请求数与工具裁决。这样把“主会话没处理”与“provider 已收到但没返回”明确分开，不引入新的权限入口或配置。

分类 worker 仅在失败时输出静态阶段名、排队耗时和总耗时；不记录命令、上下文或模型响应。阶段在现有 await 边界更新，诊断不参与授权决策。

## Risk

虚拟时间和本地 HTTP 任务调度可能有竞态；先等待 mock server 的响应屏障后再推进时钟，避免把请求准备时间混入 provider 延迟。
