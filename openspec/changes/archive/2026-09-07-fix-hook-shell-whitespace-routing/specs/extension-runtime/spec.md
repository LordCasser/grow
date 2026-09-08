## ADDED Requirements

### Requirement: Hook shell routing recognizes command separators
命令 Hook SHALL 将包含普通空格、tab 或 LF 的命令文本交由现有 shell 执行；去重 SHALL 使用相同路由判定。其他无 shell 语法的路径仍直接执行。

#### Scenario: Tab 参数与多行命令
- **WHEN** 命令用 tab 分隔参数或 LF 分隔命令
- **THEN** 经 shell 执行，不把整段文本当作相对文件路径。

#### Scenario: 跨来源 shell 去重
- **WHEN** 相同 tab 或 LF 命令来自不同 source_dir
- **THEN** 按 shell 命令既有 first-wins 规则只保留首项。
