## ADDED Requirements

### Requirement: Assistant copy formats only the selected message
/copy N SHALL 从最新 assistant 消息倒序选择，找到目标后停止扫描并只格式化目标消息，不为选择创建所有历史正文副本。

#### Scenario: Selected message among mixed blocks
- **WHEN** scrollback 混有用户和系统块，N 是有效 assistant 序号
- **THEN** 只按 assistant 消息倒序计数并复制对应内容。

#### Scenario: Invalid copy index
- **WHEN** Action 的 N 为零或超过 assistant 消息数量
- **THEN** 返回明确提示，不 panic 且不输出文件或剪贴板。
