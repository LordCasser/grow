## ADDED Requirements

### Requirement: macOS clipboard encoded images have a read budget
macOS剪贴板图片读取 SHALL 限制编码数据为50,000,000字节；空结果保持无图片语义，超限返回错误。

#### Scenario: Native image exceeds the budget
- **WHEN** NSData报告长度超过预算
- **THEN** 在分配Rust图片buffer和复制前拒绝，不把超限当作不可用转入脚本回退。

#### Scenario: Fallback image file exceeds the budget
- **WHEN** 脚本回退图片文件读取超出预算
- **THEN** 实际最多读取预算加1字节后报错，临时目录按既有所有权释放。

#### Scenario: Image fits the budget
- **WHEN** 编码数据非空且不超过预算
- **THEN** 保留全部数据与既有MIME/图片类型优先级。
