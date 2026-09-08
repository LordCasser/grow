## ADDED Requirements

### Requirement: Copy files commit private content atomically
显式复制文件和默认备份 SHALL 使用同目录临时文件完整写入并同步后原子提交；提交前失败 SHALL 保留旧内容并清理临时文件。Unix 临时文件及成功目标 SHALL 为 0600。

#### Scenario: Partial copy file write fails
- **WHEN** 临时文件部分写入后发生错误
- **THEN** 原目标内容和权限不变，临时文件清理。

#### Scenario: Previously public copy file
- **WHEN** 原目标为 0644 且新复制成功
- **THEN** 新目标包含完整文本且权限收紧为 0600。

#### Scenario: Symbolic link copy target
- **WHEN** 路径是现存普通文件的符号链接
- **THEN** 保留链接并更新其目标；悬空链接、非普通或只读目标报错。
