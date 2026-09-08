# Design
保留 public recover() 供主动恢复；私有 recover_from(expected: Option<&McpService>) 共享已有状态锁和握手路径。Some 只在 Arc::ptr_eq 时重置 Ready，None 保留主动恢复策略，非 Ready 均加入既有握手。recover_and_retry 传递最初 call_tool 使用的服务，不增加标识符或新状态。

真实 loopback HTTP fixture 用 Notify 挂住第一次工具调用，在另一路 recover 完成后释放旧错误。期望重试成功、工具调用总数2、initialize总数2（初始+已完成恢复），不出现第三次握手。
