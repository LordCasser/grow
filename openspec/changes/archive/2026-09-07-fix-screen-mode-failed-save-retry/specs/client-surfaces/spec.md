## ADDED Requirements

### Requirement: Failed screen mode persistence remains retryable
屏幕模式未配置时的保存失败 SHALL 恢复未配置状态，不把显示默认值记成已配置值。再次选择目标模式 SHALL 能重新发起保存。

#### Scenario: Failed initial fullscreen selection
- **WHEN** 原始 screen_mode 缺失，选择 Fullscreen 后保存失败
- **THEN** 回滚后仍为未配置，再次选择 Fullscreen 产生新的保存请求。
