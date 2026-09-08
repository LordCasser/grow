# Design
初始化 guard 覆盖握手和结果提交锁等待；取得结果写入锁后才 disarm，随后同步写 Ready/Pending/Empty。回归使用 ACP 永久 pending invoker，暂停时钟触发握手超时，持状态锁让结果提交挂起，释放锁后取消 future，验证 Pending 恢复。

取消时锁仍占用的 best-effort 恢复失败是独立债务，不在本项扩大为整个状态锁改造。
