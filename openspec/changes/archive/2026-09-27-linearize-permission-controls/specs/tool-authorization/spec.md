## MODIFIED Requirements

### Requirement: Permission controls revoke stale pending requests

ResetState SHALL 撤销全部旧权限请求并清空状态；ReleaseChild SHALL 撤销对应 child 请求并释放域状态；主权限模式改变 SHALL 撤销旧 primary 请求而不改变 child mode。控制调用完成 SHALL 表示 actor 已提交对应状态转换与撤销；在此后完成的旧裁决 SHALL NOT 授权调用或写入 remembered grant。新增 remembered deny SHALL 撤销同域旧判断。调用方离开后迟到的模型/人工允许 SHALL NOT 授权调用。Shutdown SHALL 取消并等待全部已接纳请求及状态写入，再关闭权限 audit 流。

#### Scenario: Control overlaps an old classifier completion
- **WHEN** classifier 在同一轮调度中触发 ResetState、ReleaseChild 或主 mode change，并准备返回允许
- **THEN** 控制完成前 actor 已撤销对应旧请求，该允许不能成为请求结果。

#### Scenario: Reset or child release during judgment
- **WHEN** 请求等待模型或人工回应时发生对应 ResetState 或 ReleaseChild
- **THEN** 当前调用结束为取消，迟到允许不能重新创建授权状态。

#### Scenario: Primary mode changes while a child waits
- **WHEN** 一个 child 正在等裁决且主 mode 被更新
- **THEN** 更新完成后对新的 primary 请求生效，旧 primary 请求被撤销，child 沿冻结的独立模式继续。

#### Scenario: Child release is isolated
- **WHEN** 一个 child 被释放而另一个 child 或主会话正在裁决
- **THEN** 仅被释放 child 的旧请求被撤销，其他权限域继续运行。

#### Scenario: Control completion precedes new request
- **WHEN** 模式切换或状态重置完成后提交新请求
- **THEN** 新请求使用已提交的模式或清空后的状态。

#### Scenario: Shutdown with multiple pending decisions
- **WHEN** 多个裁决或提示在途时关闭权限系统
- **THEN** 所有已接纳请求被取消，最终审计事件与写入处理结束后才确认关闭。
