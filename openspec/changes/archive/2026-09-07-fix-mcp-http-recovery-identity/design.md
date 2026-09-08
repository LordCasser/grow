# Design
保存 recover 的 Result，同步读取禁用列表，再持 McpState 锁同时核对 get_client Arc 身份和 HTTP/SSE 配置。最后锁内无 await，失效返回 RecoveryError::Superseded，不再进入下一次重试。缺失或非 HTTP 客户端也属于旧 HTTP 调度已失效。失效路径只清理所捕获客户端的监听器，不取得替代客户端句柄。真实错误在身份确认后映射 Failed。

不使用全局 generation，因为其他服务器变化不应否定保留客户端。该检查不保证磁盘配置在同步读取之后永远不变，也不撤销 recover 期间已产生、由 dispatcher 身份校验的事件。
