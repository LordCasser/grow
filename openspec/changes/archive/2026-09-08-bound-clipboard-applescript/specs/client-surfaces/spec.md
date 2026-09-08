## ADDED Requirements

### Requirement: Clipboard image scripts have bounded process lifetimes
macOS附件和图片AppleScript调用 SHALL 限制执行为5秒、stdout和stderr各1MiB，超限不得作为有效结果。已有进程组 SHALL 在调用结束时回收，输出收尾 SHALL 有期限。

#### Scenario: Script hangs or floods output
- **WHEN** 图片或附件脚本超过执行时限或任一输出限额
- **THEN** 返回错误并终止进程组，不继续无限收集输出。

#### Scenario: Leader exits with descendants holding pipes
- **WHEN** 脚本leader退出但后代仍持有管道
- **THEN** 回收拥有的进程组，管道收尾最多额外等待300ms。

#### Scenario: Normal script result
- **WHEN** 脚本在预算内结束
- **THEN** 保留完整stdout/stderr和退出状态，既有结果解析与错误报告继续有效。

#### Scenario: Process group cleanup fails
- **WHEN** 操作系统拒绝进程组清理
- **THEN** 返回清理错误，不宣称成功；已有执行失败原因同时保留。
