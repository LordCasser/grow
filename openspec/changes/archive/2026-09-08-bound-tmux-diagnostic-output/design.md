## Design
每条管道通过 Read::take(cap+1) 收集，长度大于cap返回错误，并用共享 AtomicBool 通知主等待循环。等待循环在 try_wait 前检查标记，沿用 terminate_tmux_tree 终止并回收；若进程先退出，则已有 drain 接收路径返回超限错误。保持独立stdout/stderr限额，总保留内容不随生产速度增长。

为真实子进程回归将 Command 执行边界抽为私有函数，生产仍使用既有build_tmux_command，不通过修改全局PATH替换tmux。不引入新的通用进程框架。

## Validation
验证恰好上限的内容保留、任一流超一字节报错及标记；真实子进程大量输出后继续等待，要求在主期限前结束。保留近期限成功/子进程树清理与解析测试。
