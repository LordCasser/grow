# Why
命令 Hook 的组终止只在 timeout await 返回之后执行，取消 future 跳过该段。Unix ProcessGroup Drop 不终止组，child kill_on_drop 仅覆盖直接子进程；没有 session scope 时甚至未建立组，孙进程会继续执行。

# What Changes
执行期守卫在异常退出或取消时终止进程组；组创建不依赖 scope，scope 只负责额外的会话级登记。

# Impact
保留成功等待完成后的既有行为；建立组失败仍记录并退化为直接子进程 kill_on_drop。
