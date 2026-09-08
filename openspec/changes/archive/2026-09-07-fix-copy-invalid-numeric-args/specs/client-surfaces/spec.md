## ADDED Requirements

### Requirement: Copy rejects invalid numeric indices instead of writing files
/copy 的首个参数若为带单个可选正负号的 ASCII 整数字面量，SHALL 作为序号解释；零、负数或超范围值 SHALL 返回错误，不得回退为文件路径。

#### Scenario: Invalid integer with optional destination
- **WHEN** 输入为负数、零或超范围整数字面量，可能带后续文件参数
- **THEN** 解析返回错误，不创建复制文件 Action。

#### Scenario: Explicit numeric filename
- **WHEN** 文件名通过 ./ 前缀或扩展名明确为路径
- **THEN** 继续按文件目标解析，合法正整数序号也保持原语义。
