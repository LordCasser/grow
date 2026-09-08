## ADDED Requirements

### Requirement: Relative executable hook deduplication includes its base directory
直接执行的相对 Hook 命令去重 SHALL 包含 source_dir，不能合并来自不同目录的同名相对脚本；去重与执行 SHALL 共用 shell 路由判定。shell 命令与绝对路径保持既有跨目录去重规则。

#### Scenario: 多目录同名脚本
- **WHEN** 两个 Hook 命令文本相同且为直接相对路径，但 source_dir 不同
- **THEN** 两个 Hook 均保留。

#### Scenario: 同一 workspace 的 shell 命令
- **WHEN** 同一 shell 命令来自不同 source_dir
- **THEN** 仍按既有 first-wins 去重，不因 source_dir 重复执行。
