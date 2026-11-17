# 检查方法

沿 Shell 的 session/load、分页读取、resident reconnect 和挂起树释放追到 Pager。对每类状态找出权威、加载响应或通知、消费 reducer 以及是否依赖某一正文页。分别检查 published prefix、未发布尾部、视图空白、已有 live 状态、缺失展示、可选响应元数据省略。

代码推演与断言现有行为的临时探针分开记录。探针使用既有 storage/ACP/Pager 测试入口和独立夹具，审计结束后移除。修复由对应 owner 按 backlog 分开收敛。
