## ADDED Requirements

### Requirement: Placeholder image caps bound actual reads
占位图片文件加载 SHALL 对实际读取执行预算加1字节上限，而非仅在完整读取后检查；现有授权、扩展名和MIME验证继续适用。

#### Scenario: File grows after size inspection
- **WHEN** 前置文件大小检查后内容超出预算
- **THEN** 最多读取预算加1字节并返回TooLarge，不继续读取完整内容。

#### Scenario: Exact budget and read failure
- **WHEN** 图片恰好在预算内或读取发生I/O错误
- **THEN** 分别保留完整有效图片或返回既有ReadFailed分类。

#### Scenario: Reporting observed excess
- **WHEN** 有界读取发现超限
- **THEN** 错误中的actual表示已观察字节，不宣称它是完整文件长度。
