# Why
stdio 握手消耗唯一子进程 transport 后不可重建，取消守卫的 restore 为 None，与正常完成后 disarm 的状态重合。因此取消不清理 Initializing，也不通知等待者，后续调用只能反复等待超时。

# What Changes
守卫保存可选的取消目标状态：Some(Pending) 或 Some(Empty)，仅 None 表示已撤销。

# Impact
stdio 取消收敛到 Empty；HTTP/ACP 继续 Pending 重试，不改变锁竞争 best-effort 策略。
