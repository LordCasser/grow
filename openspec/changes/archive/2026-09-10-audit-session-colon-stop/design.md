## Context

Timeline 保存 assistant/tool 事实，Sampler 解析 Messages 流，ChatState 管理 native continuation 与 portable 边界，sampling-types 生成实际请求，Shell 处理正常完成与继续。

## Goals / Non-Goals

核对截图对应的两次响应及此前一次同类停顿，区分可证明的协议/状态事实和模型行为的因果推测。无需线上重新执行原任务。

## Decisions

按 Timeline observation 的 chunks 读取保存的 request/response，核对字节数、尾帧、工具定义和实际调用配对。复现调用当前源码构建的 sampling-types 库，构造与 ChatState 缺失 native 回退一致的 prefix 切点；不向模型或原 session 发送请求。

只记录确认的缺口，不在审计中设计或实施新的完成判断、自动续接或工具历史投影机制。

## Risks / Trade-offs

实际 wire 与确定性投影可以证明配对缺失，不能证明这就是每次模型提前结束的充分原因。首个停顿请求没有孤立 tool_result，也必须保留为限制证据。
