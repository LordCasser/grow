## ADDED Requirements

### Requirement: Clipboard statistics count Unicode characters
共享剪贴板反馈的字符数 SHALL 按 Unicode scalar value 计算，不得将 UTF-8 字节数标为字符数；字符与行数单位 SHALL 使用正确单复数。

#### Scenario: Multibyte text
- **WHEN** 复制文本包含中文或 emoji
- **THEN** 每个 Unicode 标量计为一个字符，不按编码字节数计数。

#### Scenario: Empty and multiline text
- **WHEN** 文本为空或包含换行、组合字符
- **THEN** 字符计数覆盖全部 Unicode 标量，行数仍按 str::lines，空文本为零字符零行。
