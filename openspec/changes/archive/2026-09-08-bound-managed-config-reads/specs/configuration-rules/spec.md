## ADDED Requirements

### Requirement: Managed config reads obey byte budgets
托管配置源读取 SHALL 以实际读取量执行4 MiB上限；发布与回滚前验证 SHALL 以计划输出字节长度为上限，回滚后验证 SHALL 以原始内容长度为上限。允许读取一个额外字节以检测超限，超限 SHALL 返回错误而非接受截断内容。

#### Scenario: Source grows beyond metadata size
- **WHEN** 元数据检查后读取到超过4 MiB的源内容
- **THEN** 在最多读取4 MiB加1字节后拒绝，不继续收集。

#### Scenario: Published file grows beyond planned length
- **WHEN** 验证读取的内容超过计划长度
- **THEN** 有界拒绝，并遵循既有冲突恢复契约保留外部内容。

#### Scenario: Exact budget
- **WHEN** 内容长度恰好等于读取预算
- **THEN** 返回完整内容，继续已有内容与权限检查。
