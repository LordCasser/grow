## ADDED Requirements

### Requirement: MCP image evidence survives text-output truncation

主工具结果中的 MCP 图片 SHALL 先与会被截断的文本分离，再按现有图片验证、正规化与预算进入模型可见附件；已接纳的附件顺序和省略说明 SHALL 可由 Timeline 恢复的 Surface 重建。工具结果本体与附件 SHALL 不因 direct extension 调用而混入另一条会话。

#### Scenario: Long mixed result is restored
- **WHEN** MCP 结果含超长文本、结构化内容和多张有效图片，文本预览被截断后会话恢复或切换模型
- **THEN** 截断正文、完整输出指针和预算内图片各保留一次且顺序一致；恢复/portable 请求不把图片退化为 base64 字符串。

#### Scenario: Rejected image is explicit
- **WHEN** 一个图片附件因类型、解码或预算被拒收
- **THEN** Surface 有明确替代说明，后续有效附件仍按原序处理，不产生损坏的图片 part。
