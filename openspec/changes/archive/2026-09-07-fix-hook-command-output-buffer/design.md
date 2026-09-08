# Design
私有 capture_output 使用固定读取块持续 read，每次仅追加剩余容量，超出后继续读并丢弃。保留 MAX_OUTPUT_BYTES+1 使既有 truncate_output 区分恰好达到上限与超出。stdout/stderr 与 child.wait 用 try_join 并发，外层与 stdin 写入 join，全部在原 timeout 内；stdout/stderr 从 child 取出后不得再用 wait_with_output。

输出日志仅能标明 captured_bytes，不再把保留长度称作总输出长度。测试覆盖空、恰好上限、多块超量输出及写端完整完成，既有真实子进程大stdin/输出回归验证管道不死锁。
