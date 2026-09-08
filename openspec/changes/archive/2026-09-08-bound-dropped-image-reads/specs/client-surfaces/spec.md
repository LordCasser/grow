## ADDED Requirements

### Requirement: Dropped image reads have an encoded byte budget
拖拽及路径粘贴图片入口 SHALL 在图片识别前限制编码数据为50,000,000字节，实际最多读取预算加1字节；打开的来源必须是普通文件。

#### Scenario: Oversized or growing image file
- **WHEN** 文件长度或实际读取超过预算
- **THEN** 不生成图片附件，按既有规则保留普通文件路径回退。

#### Scenario: Valid image within budget
- **WHEN** 普通图片文件非空且未超限
- **THEN** 保留原有图片识别、维度和数据；合法符号链接拖入语义保持。
