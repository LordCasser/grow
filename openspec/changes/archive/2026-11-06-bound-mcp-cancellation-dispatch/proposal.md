# Why

2.6.2候选核对确认，Grow的 `CancelMcpCallOnDrop` 已发送原request id的取消，但它spawn的通知任务没有等待上限。锁定rmcp3.5.1 `Peer::send_notification` 既等有界队列入队，也等transport发送结果；peer不读或发送受阻可以保留Grow任务及service clone。用户授权按证据吸收上游3秒限界。

# What Changes

给Grow拥有的best-effort取消派发任务一个固定3秒等待预算，覆盖入队和发送结果等待；超时只结束本次派发等待并记录未确认，不能称已成功取消远端动作。原工具调用的绝对deadline、不重试取消、原service/request identity及成功调用不发取消保持。

# Impact

只修改MCP调用取消owner及实际受阻transport回归，不改MCP初始化/继承/名称政策，不取消或替换共享client。SDK内部已经排队的transport发送有自己的lifecycle，Grow结束等待不承诺撤回wire消息或释放SDK所有transport工作。
