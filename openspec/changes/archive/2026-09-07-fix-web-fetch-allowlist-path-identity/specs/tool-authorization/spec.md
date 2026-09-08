# Delta

## ADDED Requirements

### Requirement: Web allowlist preserves path identity
web_fetch 静态允许列表 SHALL 仅对主机执行大小写、www 前缀和尾点规范化；路径匹配 SHALL 保留大小写和末尾点，并以完整路径段作为前缀边界。

#### Scenario: 不同路径不共享静态许可
- **WHEN** 配置 example.com/Docs 或 example.com/docs.
- **THEN** 对应路径及其子路径匹配，但 /docs 不因路径规范化而获得该条目的静态许可。

#### Scenario: 带路径主机规范化
- **WHEN** 配置 WWW.Example.COM./Docs
- **THEN** example.com/Docs 匹配，example.com/docs 不匹配。
