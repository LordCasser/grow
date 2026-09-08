# Why
启用滚动日志后第一次记录在输入线程File::create目标，显式FIFO路径可能等待reader并冻结UI。日志仅应向普通文件写JSONL。

# What Changes
Unix使用O_NONBLOCK打开并检查同句柄regular file，检查后才截断普通文件。非普通目标走已有open失败禁用状态。

# Impact
仅scroll log目标打开；保留lazy-open、普通符号链接及显式普通文件覆盖行为。不实现日志增长配额、异步writer或删除诊断功能。
