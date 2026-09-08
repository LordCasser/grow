## ADDED Requirements

### Requirement: Suggestion generation follows receiver lifetime
AI Suggest 与 Prompt Suggest 后台任务 SHALL 在结果接收端关闭时停止等待并丢弃生成 future，释放任务 activity；关闭已发生时 SHALL 优先处理关闭而不 poll 生成 future。正常完成 SHALL 交付结果。

#### Scenario: 请求超时或被丢弃
- **WHEN** 接收端在生成过程中关闭
- **THEN** 生成 future 被丢弃，已有 Sideband 取消清理生效。

#### Scenario: 开始前关闭
- **WHEN** 后台任务首次运行前接收端已关闭
- **THEN** 不开始生成。

#### Scenario: 正常交付
- **WHEN** 生成完成且接收端仍存在
- **THEN** 接收端取得生成结果。
