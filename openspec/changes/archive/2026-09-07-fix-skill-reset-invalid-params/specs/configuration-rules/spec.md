## ADDED Requirements

### Requirement: Skill reset and config reject invalid parameters
技能 reset 与 config 接口 SHALL 在执行配置读写或发现之前校验可选 cwd 参数，类型错误 SHALL 返回 invalid_params，不回退为有效默认请求。

#### Scenario: Invalid reset parameters
- **WHEN** reset 或 config 收到 null、数组或非字符串且非 null 的 cwd
- **THEN** 返回 invalid_params，不执行配置清空或技能发现。

#### Scenario: Default cwd request
- **WHEN** 请求为空对象或 cwd:null
- **THEN** 保持有效默认 cwd 请求，字符串 cwd 也正常接受。
