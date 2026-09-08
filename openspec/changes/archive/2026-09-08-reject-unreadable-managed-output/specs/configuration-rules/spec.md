## ADDED Requirements

### Requirement: Managed plans preserve source readability
托管配置规划 SHALL 拒绝最终内容超过4 MiB的计划及正文或注释前缀含NUL的请求，不写出已知违反源读取规则的文件。

#### Scenario: Adding a block exceeds the source limit
- **WHEN** 渲染后的完整输出超过4 MiB，包括原文与标记
- **THEN** 规划返回明确超限错误，原文件与缺失状态保持，不生成事务文件。

#### Scenario: Exact output limit
- **WHEN** 输出恰好4 MiB且其他验证通过
- **THEN** 允许应用，后续可正常读取和幂等规划。

#### Scenario: NUL in requested body
- **WHEN** 任一请求条目正文包含NUL
- **THEN** 在规划写入前返回无效请求错误，保持源不变。

#### Scenario: NUL in comment prefix
- **WHEN** 构造的注释前缀包含NUL
- **THEN** 返回无效请求错误，不允许该前缀进入渲染。
