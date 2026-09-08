## ADDED Requirements

### Requirement: Image file URIs preserve literal path bytes
图片附件file URI SHALL 通过标准file-path URL转换生成与解析，百分号只解码一次；不在literal路径和解码路径之间猜测。

#### Scenario: Literal percent and reserved characters
- **WHEN** 图片文件名含字面%20、%2F、空格、#、?或Unicode
- **THEN** URI往返保留同一路径，去重不把不同文件合并。

#### Scenario: URI is not a plain local file identity
- **WHEN** 输入非file URI，或带query/fragment
- **THEN** 不将其当成本地图片规范路径用于去重。
