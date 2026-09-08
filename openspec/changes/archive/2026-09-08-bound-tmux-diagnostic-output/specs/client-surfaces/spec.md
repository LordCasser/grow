## ADDED Requirements

### Requirement: Tmux diagnostic output is bounded
共享 tmux 查询 SHALL 对 stdout 和 stderr 分别限制64 KiB，不将超限截断结果作为有效诊断值。

#### Scenario: A pipe exceeds its limit
- **WHEN** 任一管道超过64 KiB
- **THEN** 返回输出超限错误，等待中的进程树被终止并回收，不继续无限收集。

#### Scenario: Output fits the limit
- **WHEN** 输出不超过每流限额且进程在期限内完成
- **THEN** 保留全部输出并按原有规则解析，退出后管道清理窗口保持。
