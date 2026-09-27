## ADDED Requirements

### Requirement: Permission waits are request-local

主会话和子 Agent 的权限请求 SHALL 独立等待模型或人工回应；等待中的请求 SHALL NOT 阻塞其他请求的本地策略检查、独立模型裁决或权限控制命令。同一权限域的状态提交 SHALL 有序合并到当前状态，不得以旧快照覆盖其他请求的授权、拒绝计数或历史。根权限文件写入 SHALL 串行保存最新状态，子权限域 SHALL 不落入根权限文件。

#### Scenario: One child judgment is slow
- **WHEN** 子 Agent A 的模型裁决已发出但未结束，而 B 或主会话提交独立权限请求
- **THEN** 后者可以完成本地决策或发出自己的 Sideband，无需等待 A 的模型终态。

#### Scenario: Human permission is pending
- **WHEN** 一个请求等待人工回应，另一个请求可通过本地规则或 Auto 判断处理
- **THEN** 后者继续处理，不受前者的人工交互期限约束。

#### Scenario: Concurrent decisions update one scope
- **WHEN** 同一权限域内多个独立请求完成并提交计数、历史或 remembered grant
- **THEN** 所有当前有效更新按完成顺序合并，主/子和不同子域之间不共享授权。

### Requirement: Permission controls revoke stale pending requests

ResetState SHALL 立即撤销全部旧权限请求并清空状态；ReleaseChild SHALL 撤销对应 child 请求并释放域状态；主权限模式改变 SHALL 撤销旧 primary 请求而不改变 child mode。新增 remembered deny SHALL 撤销同域旧判断。撤销或调用方离开后，迟到的模型/人工允许 SHALL NOT 授权调用或写入 remembered grant。Shutdown SHALL 取消并等待全部已接纳请求及状态写入，再关闭权限 audit 流。

#### Scenario: Reset or child release during judgment
- **WHEN** 请求等待模型或人工回应时发生对应 ResetState 或 ReleaseChild
- **THEN** 当前调用结束为取消，迟到允许不能重新创建授权状态。

#### Scenario: Primary mode changes while a child waits
- **WHEN** 一个 child 正在等裁决且主 mode 被更新
- **THEN** 更新立即对新的 primary 请求生效，旧 primary 请求被撤销，child 沿冻结的独立模式继续。

#### Scenario: Shutdown with multiple pending decisions
- **WHEN** 多个裁决或提示在途时关闭权限系统
- **THEN** 所有已接纳请求被取消，最终审计事件与写入处理结束后才确认关闭。

## MODIFIED Requirements

### Requirement: Auto model permission judgments obey one end-to-end deadline

主会话和子 Agent 的 Auto 模型权限裁决 SHALL 从请求提交给主会话分类通道开始，共享一个有界总期限；该期限 SHALL 覆盖派发、准备、模型尝试与结果结算。首次尝试 SHALL 可使用全部剩余期限，不得仅为预留重试时间而提前取消仍在运行的请求。实际返回无效结构或可恢复 provider 错误后 SHALL 最多重试一次且不得延长总期限。超过期限或请求方已离开后 SHALL 不采纳允许结果。

#### Scenario: 较慢的首次结果仍在总期限内
- **WHEN** 首次模型尝试超过旧的平均半额但在当前总期限内返回有效裁决
- **THEN** 裁决被使用，不因预留第二次尝试预算而提前取消。

#### Scenario: 排队或准备耗尽期限
- **WHEN** 分类请求在派发或 Sideband 准备期间耗尽总期限
- **THEN** 该精确调用不获得允许；迟到响应不能改变结果或写入许可。

#### Scenario: 子 Agent 与主会话超时后续
- **WHEN** 子 Agent 的 Auto 模型裁决超时
- **THEN** 当前工具调用失败且不打开人工提示，子 Agent 可继续其他调用；主会话自己的 Auto 分类超时仍进入既有的人工提示后续路径。
