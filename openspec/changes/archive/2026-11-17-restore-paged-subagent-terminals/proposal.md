# Why

恢复分页会话时，Pager 用 canonical Spawned 创建子任务视图，却没有应用同一份所有权数据里的 Ended。终态正文页尚未加载时，已结算子任务会显示为运行中，错误地提供取消操作，并使主轮次显示正在等待子任务。

# What Changes

对子任务恢复批次已选中的直属子任务，从同一 cut 的 canonical 记录补齐任务状态。故障样本在恢复前已有 5 个 Completed 和 2 个 Cancelled，挂起检查点只含根会话。实际缺口在 Pager 的状态投影。
