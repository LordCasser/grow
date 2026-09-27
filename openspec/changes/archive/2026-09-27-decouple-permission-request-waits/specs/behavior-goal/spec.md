## ADDED Requirements

### Requirement: Independent permission Sidebands coexist with live foreground attempts

Goal 用量准入 SHALL 允许独立后台权限 Sideband 与同 owner 当前 epoch、同一 active Goal 内仍在运行的前台或后台 attempt 并行。已返回但未确认结算、已认领结算、旧 epoch 的 attempt SHALL 继续阻止对应 owner 后续准入，关闭的 Goal SHALL 不接纳新请求。并发 attempt 的使用量 SHALL 各自确认且只计一次。

#### Scenario: Child permission arrives during foreground inference
- **WHEN** 主模型 attempt 正在运行且子 Agent 提交需主会话裁决的权限请求
- **THEN** 权限 Sideband 可以发出 provider 请求，无需等待主模型结束；运行中的权限 Sideband 也不阻止主会话的独立模型准入。

#### Scenario: A concurrent attempt returned but usage is unsettled
- **WHEN** 并发 attempt 已返回或开始结算但 owner 尚未确认其用量
- **THEN** 新准入等待对应结算；epoch 改变或 Goal 关闭后不利用后台身份绕过边界。
