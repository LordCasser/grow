# Route SDK MCP reverse requests to their registration owner

## Why

Backlog MCP-F2 的真实producer `GatewayAcpInvoker`只发送`serverId`和`message`，leader既找不到目标连接也找不到session，带id请求会被丢弃。仅补sessionId会把SDK调用交给当前driver，而SDK工具实际属于声明它的注册连接。当前new/load入口还允许caller预置`grow/leaderClientId`，该透传值不能直接作为注册者身份。

## What changes

leader在new/load的有效请求元数据中用实际连接ClientId覆盖路由身份；Shell在本次SDK注册时把sessionId和该owner绑定到既有GatewayAcpInvoker，reverse params携带同一身份，复用leader已有target单播。不同连接同serverId互不干扰，driver切换不改变注册者，注册者断线不回退。常驻load保原SDK注册；cold load按新请求重新声明和绑定；继承的已连接SDK client保原invoker和owner。

## Impact

leader ingress与Shell session SDK注册/producer各自承担一项独立承诺，按顺序实现和验收。复用现metadata、ClientId、gateway及target路由，不新增registry、wire方法、权限框架或持久ClientId；SDK entry与MCP库无需为同批次的共同owner新增字段。MCP-F1初始化后资格、标准MCP source合并、重复声明F3、目录名称限制和elicitation均分别处理。
