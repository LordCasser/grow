# Design
reset_transport 接收可选的 expected service，超时调用必须传入 Some(本次服务)。在同一状态锁内比较 Arc 身份后推进 revision 和写 Pending。None 保留低层强制 reset 测试使用，不由生产超时调用传入。原 replace_state helper 仅服务该调用，合并入原子 reset，保留 init_done 唤醒。

复用真实 HTTP DeferredError fixture，挂住旧请求，先 recover 新服务，然后等待旧请求超时。断言新 Ready/服务 Arc 保持、握手次数不增加、工具只调用一次。
