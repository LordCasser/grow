## ADDED Requirements

### Requirement: Skill extension reload has a bounded execution boundary
技能扩展重载 SHALL 将同步扫描与异步请求执行隔离，限制仍未退出的扫描数量，且截止时间包含等待执行资格的时间。请求超时不得被表述为阻塞文件调用已中断。

#### Scenario: Scan remains blocked after timeout
- **WHEN** 一次扫描未退出但调用者已超时，随后又有重载请求
- **THEN** 旧扫描继续占用执行资格，后续请求不得无限创建扫描任务，仍可按截止时间失败。

### Requirement: Skill reload failure is distinct from empty discovery
技能扩展 SHALL 将重载超时或 worker 失败返回为错误，不转换为成功空列表；已完成的配置保存 SHALL 不被宣称回滚。

#### Scenario: Reload fails after settings save
- **WHEN** 添加、移除或 reset 已完成保存，后续重载失败
- **THEN** 返回明确的重载失败，保留已保存配置，不返回空成功技能目录。

#### Scenario: Empty discovery succeeds
- **WHEN** 扫描正常完成且未找到技能
- **THEN** 返回正常的空技能列表。
