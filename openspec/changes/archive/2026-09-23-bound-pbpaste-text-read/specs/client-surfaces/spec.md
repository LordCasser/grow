## ADDED Requirements

### Requirement: macOS clipboard text subprocesses have bounded execution
macOS `pbpaste -Prefer txt` 文本读取 SHALL 使用有界子进程运行器：执行时间最多 5 秒，stdout 和 stderr 各最多 1 MiB，并在调用结束时回收其所属进程组；超时、任一流超限、启动或非零退出 SHALL 返回读取错误。

#### Scenario: Text command hangs or exceeds an output allowance
- **WHEN** `pbpaste` 超过 5 秒，或 stdout/stderr 任一流超过 1 MiB
- **THEN** 读取失败且所属进程组被回收，不继续无限等待或收集输出。

#### Scenario: Text command succeeds within budget
- **WHEN** `pbpaste` 在期限内以零状态退出且两条流均在限额内
- **THEN** 保留原有语义：空 stdout 返回无文本，非空 stdout 按 UTF-8 有损解码返回文本。
