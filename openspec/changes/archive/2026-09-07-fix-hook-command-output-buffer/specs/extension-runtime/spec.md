## ADDED Requirements

### Requirement: Command hook output capture is bounded
命令 Hook SHALL 在读取期间限制每路 stdout/stderr 缓存为 64 KiB 加一个截断检测字节，并持续排空超出数据；stdin 写入与两路输出读取 SHALL 并发且受原执行超时约束。

#### Scenario: 输出超过上限
- **WHEN** 子进程输出远超 64 KiB
- **THEN** 保留前缀和截断标记，缓存不随输出总量增长，输出管道仍被排空。

#### Scenario: 输出恰好达到上限
- **WHEN** 单路输出恰好 64 KiB
- **THEN** 完整保留且不添加截断标记。
