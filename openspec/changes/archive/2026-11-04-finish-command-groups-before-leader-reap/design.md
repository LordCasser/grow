## Ownership

Unix 的 PID 在未 reap 的子进程存续期间不能被复用。以 libc waitid(P_PID, WEXITED|WNOHANG|WNOWAIT) 观察终态，在仍持有未 reap leader 的时候 kill group，再由 Tokio Child::try_wait reap。没有退出则不调用会竞争 reap 的 try_wait。已 reap 的 child 仅返回既有缓存状态，不对保存的 PGID 发信号。

显式取消在既有 SIGTERM_GRACE 内不 reap leader，grace 完后先 kill group 再 wait。正常退出也结束本命令遗留的同组后代；显式 setsid 脱离进程组不在该能力保证内。Windows 继续用 JobObject 的稳定 HANDLE，不引入 Unix PGID 逻辑。

## Verification

测试只使用本次命令记录的 PID，后代自身最多八秒；包含清理 guard，不使用全局 pgrep/kill。验证 leader TERM trap early exit、timeout 和正常 leader exit，后代忽略 TERM，组结束后 active scope owner 清除；运行 terminal 全组及 native 平台门禁。

受限回归首先发现 foreground timeout 还会在设置临时 exit_status 的同一次 sweep 被 evict，阻止下一轮组 KILL。eviction 必须同时确认 child 已 reap；命令结果可按现有时序回复，但 native cleanup owner 保留到实际回收。
