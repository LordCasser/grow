# Why
命令 Hook 声明限制输出为 64 KiB，但 wait_with_output 实际先完整收集 stdout/stderr，再 truncate_output。超时前大量输出仍可造成无界内存增长，输出显示限制并未约束读取缓存。

# What Changes
并行排空 stdout/stderr，各仅保存 64 KiB 加一个截断检测字节，继续排空多余数据直到 EOF。

# Impact
保留截断标记、超时和 stdin 并发写入，不改变 hook 决策解析策略。
