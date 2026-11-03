# 设计

在已解析的 BuiltinAction::FlushMemory 分支直接启动有活动许可的本地任务，复用 run_memory_flush 的 runtime memory gate、原子锁、冻结快照和完成通知。command loop 仅确认接纳，不等待 provider；真正结果沿现有 MemoryFlushCompleted 反馈。其它 slash 命令维持已有控制边界。
