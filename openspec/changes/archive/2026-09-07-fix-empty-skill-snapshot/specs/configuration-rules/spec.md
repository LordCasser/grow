## ADDED Requirements

### Requirement: Loaded skill bodies preserve empty snapshots
技能正文加载成功后 SHALL 保存已加载状态，包括空正文。读取已加载正文 SHALL 返回快照，不重新读取磁盘。未加载的 synthetic path SHALL 返回缺少正文错误。

#### Scenario: Empty file body frozen before disk changes
- **WHEN** 空技能正文被加载为快照，随后文件被修改或删除
- **THEN** 读取快照仍成功返回空正文。

#### Scenario: Explicit empty synthetic body
- **WHEN** synthetic path 技能携带已加载空正文
- **THEN** 返回空正文；只有缺失正文时返回错误。
