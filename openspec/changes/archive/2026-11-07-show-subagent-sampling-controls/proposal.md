# Why

主 Agent 为子 Agent 切换 model/effort 时，当前工具行只保留 Queue 标题，没有清楚的分发/完成形态；子会话只更新模型标签，缺少与人工切换相同的控制反馈。用户要求主会话同一行更新，并在子会话记录显示控制指令。

# What Changes

主会话复用现有工具行身份和紧凑通信样式，显示 Dispatching → Queued，失败留在原行。Queued 表示目标 actor 已接纳请求，模型仍在下一 closed-Step 边界切换。子会话沿原 Sampling control 投影、持久回执和终态 notice 显示真实切换；请求身份由宿主生成，不接受模型自报的 client/session 元数据。

# Impact

Pager 拥有主工具行投影；Shell 拥有工具来源控制身份和子 actor 回执。沿现有 API、工具输出和 Session ControlState，不新增 wire 字段、模型池、通用控制框架或执行权限。模型池保持独立设计批准门槛。
