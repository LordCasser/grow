# Design

当前 `call_tool_cancel_aware_until` 用一个绝对deadline包发送和响应，guard只保存最初 `McpService` Arc和request id，drop后独立spawn发送一次 `notify_cancelled`。保留这条职责；仅在该spawn拥有的通知await外加固定3秒 `tokio::time::timeout`（或等效单一绝对期限）。不把这三秒加到caller deadline，不await该cleanup阻塞工具返回。

结果分三类：通知发送成功；发送错误；等待三秒仍未确认。后两者仅记录现有debug级诊断及原request identity。不能把取消通知成功等同远端副作用已撤销，不能因发送受阻retry或换新service。常量private，可有最小private helper以便共享生产和机制验证，不新增公共控制API。

锁定SDK source已亲核：`rmcp-3.5.1/src/service.rs:835` `send_notification`→bounded tx.send→receiver.await；1537处理notification后把 `transport.send` 放进SDK send_task_set。外层timeout会结束Grow等待并释放其McpService Arc；通知若已经入队，SDKsend可能继续。这是边界说明，不通过替换transport或停止共享服务扩大本修复。

验证应使用真实rmcp service及受控backpressure transport。握手成功，原tools/call已到wire，随后server停止读取，使取消notification await确实pending；证明caller原deadline及时返回，Grow取消派发owner在3秒后退出（可用实际service Arc释放或现有owner观测，不能仅测试timeout helper）。顺畅传输仍只向原request发送一次；成功调用不取消；replacementservice不收旧id，保留现有三项控制。若SDK队列/发送仍继续，明确记录，不能宣称wire发出或SDK资源全释放。虚拟时间仅在握手/工具到达的因果barrier后推进，避免把backpressure前提写成sleep猜测。

当前Cargo统一窗口属Goal修复；实现者先在隔离临时候选或只读准备中收敛一个模块功能，父明确转移窗口后再集成/编译。不得与相邻MCP初始化、连接owner、名称256候选合修。
