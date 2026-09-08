# Why
HTTP 恢复在身份检查之后再次 await 配置检查，同名替换可漏过；恢复失败通过 ? 提前返回，也不会核对身份。旧任务把配置失效视作传输失败，下一次按名称重试可重置替代连接。

# What Changes
stdio/HTTP 共用 RecoveryError（重命名既有 RespawnError），HTTP 失效立即退出循环。恢复成功和失败均先在最后状态锁内核对当前客户端及 HTTP 配置，再传播结果。

# Impact
保留真实故障重试次数和 lazy recovery，保留 HTTP 初始化事件的原有所有者。
