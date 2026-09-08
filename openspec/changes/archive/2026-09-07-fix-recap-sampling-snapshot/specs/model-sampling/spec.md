## ADDED Requirements

### Requirement: Recap sampling uses one configuration snapshot
Session recap SHALL 从同一准备配置生成 client、显式模型和 context_window 预算；准备完成后的配置切换 SHALL 不把新模型或预算拼入旧 endpoint/backend 的请求。

#### Scenario: 准备后切换模型
- **WHEN** recap 已准备配置且会话切换到另一模型或 endpoint
- **THEN** 此 recap 请求仍使用准备配置的 endpoint/backend/model/window，后续 recap 才使用新配置。
