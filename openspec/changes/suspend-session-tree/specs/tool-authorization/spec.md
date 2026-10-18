## ADDED Requirements

### Requirement: Suspended tool continuations revalidate current authority

恢复 SHALL 保留工具原调用身份、最终已冻结参数及尚待用户决定的请求关联，但不得恢复旧运行实例的一次性 permit 或以挂起事实授予权限。未派发调用 SHALL 按当前 actor、工作区、工具定义和 MCP transport/trust generation 重新核对；需要用户决定时重建与原操作绑定的真实等待。挂起 SHALL 不生成 Allow/Reject 业务决定，不重跑结果未知的外部 Hook。

#### Scenario: Suspension occurs at a permission prompt
- **WHEN** 用户尚未回答原操作的权限问题就挂起退出
- **THEN** 保存待决关联，恢复后重新展示该操作的有效问题；没有答复不等于拒绝或授权。

#### Scenario: Tool or permission scope changes offline
- **WHEN** 运行环境、工具参数契约、MCP generation 或权限在离线期间改变
- **THEN** 旧 permit 不可用，恢复阻止不再合法的派发并报告原因，不静默扩大权限或重新生成另一项任务。

#### Scenario: A preflight Hook has an unknown outcome
- **WHEN** 挂起时权限或参数处理所需 Hook 已开始但结果不明
- **THEN** 保留该 occurrence 的证据并使相关操作恢复受阻，不能为取得参数或批准而盲目再运行外部 Hook。

目标入口：`crates/codegen/shell/src/session/actor/tool/{preparation,authorization,dispatch}.rs`、现有 permission/Hook owner。
