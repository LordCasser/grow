# 取证边界

以 macOS 对具体 PID 的 `sample` 和 `proc_pid_rusage(RUSAGE_INFO_V4)` 为进程内存证据，RSS、footprint、系统 Swap 和 Grow jemalloc trace 分别记录，不混加。进程 ancestry 归属到 Grow 会话后，再以子会话 canonical 工具记录核对启动命令、超时和重复执行。

只读对比临时变异副本和原工作区的环检测代码。变异副本中的 `if false` 是测试主动注入，不能当成 Solaris 正式源码缺陷，更不能通过恢复它伪造变异验证通过。Grow 源码工作树与运行中的旧二进制分开；当前源码行为不用于替代已运行实例的实际结果。

本次不引入全局资源管理器。外部测试的资源预算及可靠清理属于独立边界，记录到 backlog 后单独确定实现范围。
