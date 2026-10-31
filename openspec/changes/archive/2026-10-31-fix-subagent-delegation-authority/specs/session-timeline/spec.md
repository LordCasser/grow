## ADDED Requirements

### Requirement: Restored children retain their original birth authority

原 suspended child 重建 SHALL 从原 Spawn 的已持久化有效 capability_mode 恢复出生 RWX，并与当前直接父权限上限相交；当前定义默认或已选 profile 不得扩大原生命周期的出生权限及后代 ceiling。恢复事实缺失或无效 SHALL 阻止整树执行准入。逐调用授权仍须按当前策略重新检查，不复用旧 permit。

#### Scenario: A readonly child resumes with an all-capability profile
- **WHEN** 原 Task 未指定 capability override，child 出生为 ReadOnly，当前 profile 默认为 All，整树挂起并销毁后从持久化 checkpoint 恢复
- **THEN** child 保持原身份和 ReadOnly 出生权限，超出出生 RWX 的工具仍需当前授权，后代上限不扩大。

#### Scenario: Persisted birth authority is unavailable
- **WHEN** 原 Spawn 缺失有效 capability_mode
- **THEN** 恢复报告出生权限事实不可用并保持树执行准入关闭，不回退当前 profile 默认。
