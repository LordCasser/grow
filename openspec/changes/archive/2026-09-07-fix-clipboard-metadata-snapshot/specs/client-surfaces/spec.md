## ADDED Requirements

### Requirement: Clipboard metadata snapshots reject observed version races
macOS 剪贴板元数据快照 SHALL 在类型分类前后读取版本，仅在版本一致且类型分类可用时返回有效版本与分类。

#### Scenario: Clipboard changes during classification
- **WHEN** 类型分类前后的版本不同
- **THEN** 返回未知结果，不将旧版本绑定到新类型，也不在该调用内循环重试。

#### Scenario: Types cannot be read
- **WHEN** 类型列表不可用
- **THEN** 返回未知，不能据此确认没有图片。

#### Scenario: Stable known metadata
- **WHEN** 前后版本一致且类型可用
- **THEN** 返回该版本及共享类型规则的图片分类，保持文件 URL 优先规则。
