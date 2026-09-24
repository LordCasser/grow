# Design

History daemon 的 JoinHandle 在生产代码中保持 detached；在测试构建下增加 started/exited 原子标记，使测试能观察真实 worker 是否消费过请求，以及 Drop 发出 stop 后线程是否离开接收循环。测试提交 30,000 条较长记录，等待 worker 取走请求后 Drop，再以有界等待确认 worker 退出。

该测试只证明此 workload 与调度条件下 stop 可使 worker 最终退出，不形成一般 wall-clock 上界。现有模拟阻塞测试继续负责证明 Drop 本身不等待执行中的操作。文件模糊搜索没有对应 worker-exit hook；新增该观测机制及可控慢文件系统不属于本最小 change。
