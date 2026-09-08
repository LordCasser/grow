## ADDED Requirements

### Requirement: Clipboard hint metadata probes skip busy native readers
macOS 剪贴板提示元数据入口 SHALL 在原生粘贴读取占用串行锁时返回不可用，不等待该读取释放锁。

#### Scenario: Paste reader owns the native lock
- **WHEN** 图片提示尝试读取版本或类型，而原生剪贴板锁已被占用
- **THEN** 元数据探测立即跳过并返回未知，保留原生访问串行性。

### Requirement: Clipboard hint dedup commits classified versions only
图片提示轮询 SHALL 仅将具有有效分类版本的已处理结果用于去重，不将不可用分类视为确认无图片。

#### Scenario: Classification is temporarily unavailable
- **WHEN** cheap 探测看到新版本但分类返回未知
- **THEN** 不提交该版本，后续允许的轮询继续重试。

#### Scenario: Version changes between probe stages
- **WHEN** cheap 与非图片分类返回不同版本
- **THEN** 去重记录分类结果自身版本，不记录较早 cheap 版本。
