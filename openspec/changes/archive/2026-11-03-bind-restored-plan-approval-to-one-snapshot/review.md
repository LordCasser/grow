# 独立对抗审查

reviewer `/root/rules_recovery_review` 对 `/tmp/grow-plan-recovery-baseline-path.txt` 指向的本增量基线复核；不把整个dirty工作树diff当本项，不运行Cargo。

确认合法B2替换A时old代码二读B并无条件清成Normal，故障barrier为test-only一次性TLS信号，不使用sleep/重试。单次读取、snapshot hash和exact finish_if方向成立。

审查补出两处同源边界：request在state await后重采样身份，可能形成A正文+B身份；stale transport bit支路先改内存再等generic commit锁，失败回滚可覆盖B。分别修为显式expected/bodyhash与state/hook后核对；先取得原事务边界再CAS、局部next、ACK后publish。独立复核确认原PlanControl与恢复调用的正文字节/hash一致，不会误拒绝正常正文。

最终静态复核通过：step/state/goal锁顺序与finish_if一致，失败不改内存、不回滚别的owner，obsolete无条件helper已删除。没有新增产品API、auto retry或snapshot框架。实际测试结果由主代理记于verification。
