## ADDED Requirements

### Requirement: Transcript file export commits completed content atomically
CLI/TUI 文件导出 SHALL 先完整写入同目录临时文件并同步，再原子替换普通目标；提交前失败 SHALL 保留旧内容并清理临时文件。

#### Scenario: Partial temporary write fails
- **WHEN** 临时文件已写入部分内容后发生错误
- **THEN** 导出失败，原目标内容保持不变且临时文件清理。

#### Scenario: Existing symbolic link
- **WHEN** 目标为指向现存普通文件的符号链接
- **THEN** 提交替换链接目标，保留符号链接；悬空链接报错。

#### Scenario: Permissions and special targets
- **WHEN** 目标为已有普通文件、新文件或非普通文件
- **THEN** 已有权限保留且只读目标拒绝；Unix 新文件默认私有权限；非普通目标拒绝。
