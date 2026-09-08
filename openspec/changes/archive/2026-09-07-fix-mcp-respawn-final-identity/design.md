# Design
前置 generation/config 检查保护握手结果安装。安装后的最后提交保护实际客户端身份：监听器初始化返回后获取 McpState 锁，在 owned_clients 中核对 Arc::ptr_eq，失配即 Superseded，匹配才在同一锁内 emit_current_tools_changed 并返回成功。采用客户端身份而非全局 generation，避免其他服务器配置变化误判保留的客户端。

不清理替代客户端的监听器。失效客户端由本地 Arc 释放，监听任务弱引用不会长期保活。现有恢复循环对 Superseded 静默退出，禁止成功状态和耗尽注销。

HTTP recover 后分离的身份/配置检查属于独立边界，登记 backlog 后另行处理。
