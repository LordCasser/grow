# Delta

## ADDED Requirements

### Requirement: Cached web content follows current model budget
web_fetch SHALL 对缓存命中的文本应用当前调用的模型上下文预算，保留完整缓存供后续调用使用。

#### Scenario: 大窗口切换到小窗口
- **WHEN** 页面完整文本已缓存且模型窗口缩小后再次获取同一 URL
- **THEN** 输出按新预算裁剪，存在会话目录时保存完整文本并提供恢复路径。

#### Scenario: 再次放大窗口
- **WHEN** 小窗口调用已生成裁剪预览后，大窗口调用命中同一缓存
- **THEN** 返回符合大窗口预算的完整内容，缓存不包含前次调用的裁剪或路径。
