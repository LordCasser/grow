# 对抗性审查

已证实并纳入当前修复：TERM leader early-exit 遗留同组后代；foreground timeout 的临时终态在下一次 cleanup poll 前被驱逐。回归分别覆盖显式取消、timeout 和自然完成，不通过延长 grace/fixture 期限使测试通过。

本次 terminal 在最终组信号后、调用可能 reap 的 start_kill/try_wait/wait 之前放弃自身 ProcessGroup 强引用。已知未覆盖的是全局 ProcessScope 已在此前 upgrade Weak 并持有 Arc、随后才执行 kill 的跨线程交错；此设计早于本 change。该交错是否能在受控 OS PID/PGID 复用下误命中新组尚未验证，不能宣称本次已证明 supervisor 全路径的身份同步。按工程约束记录为独立受控故障注入门槛，不在当前命令收尾修复里重做全局 ProcessScope。

macOS 原生 descendant 回归只证明同组后代停止（消失或僵尸态），不冒称能够替代 PID 1 的最终回收；主动脱离组的后代不在本能力范围。其他平台由官方 native CI 执行现有进程树/生命周期门禁，不能把本机结果当作跨平台实测。
