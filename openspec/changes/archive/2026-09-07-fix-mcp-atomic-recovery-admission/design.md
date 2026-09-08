# Design
恢复调用持有 state 锁检查 Ready，仅该转换的拥有者构建 restorable transport、推进 revision、写 Pending 并唤醒等待者。其他恢复见非 Ready 直接进入现有 single-flight ensure_initialized。没有第二个恢复锁或新的状态机实体。

测试通过真实 ACP in-process handshake 建立 Ready，持有 state 锁把两个 recover future 排队；显式 poll 形成可重复的竞争交错，断言只推进一次 reset revision，再完成两者并确认共用服务与一次新握手。
