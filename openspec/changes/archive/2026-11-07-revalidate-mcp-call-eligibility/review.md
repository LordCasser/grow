# Parent review — accepted

本项功能与Shell调用方里程碑已验收。父亲读旧实现真实模型握手反例（disabled/remove/replace/upstream revoke/rebind/recovery仍成功发往旧A）、最终共同validator/两个调用入口和实际测试日志；原coder已终态释放source/Cargo。最终新增资格16pass、MCP回归197pass、Shell调用方13pass均actual0，包含真实call_mcp_tool的过期URL拒绝和当前endpoint一次调用。仅同一client/现工具资格允许发送，owned覆盖、实时上游授权、原deadline和no-replay保持，无新公共实体或依赖。

以下两处预审发现均已修订并核实际证据：R1沿原absolute helper，post-init锁等待计入原1秒预算；R2在进入tools/call时计received，恢复用例比较已有warmup后的增量。见[完整验证](verification.md)。

## R1：direct资格锁等待不得退还预算

触发：`McpClient::call_tool`在ensure_initialized完成后计算remaining，随后新增await等待McpState锁，释放锁后仍用`call_tool_cancel_aware(..., remaining)`创建相对deadline。锁等待期间消耗的预算被重新给到网络调用，违反原admitted absolute deadline。

修订：沿现`call_tool_cancel_aware_until`传入原deadline和timeout，不能重造相对期限。通过标准为真实post-init state-lock barrier：锁等待消耗预算后，发送/响应仍在原deadline到期；配合实际received计数区分未发送和已发未完成。原model路径已用absolute helper，不需新框架。已向原coder指出，候选终态时再核实际diff与结果。

## R2：测试计数须表达收到请求

原BridgeServer在call_delay之后加calls，取消/到期时会把已收到但未答复的调用记为0；这不足以证明发送前拒绝。fixture已改为tools/call分支一进入就记received、答复完成另计，实际断言须使用received。恢复用例的warmup已正常调用一次，应比较第二次请求不再增加，不能对整个server lifetime要求0。

通过标准：初始化期间失资格的本次请求使A/B received增量均0；有效control增量1；等待后到期的control按真实收到与否报告，不能把响应未完成冒充未发送。旧日志中计数假红与编译失败单列，不计为产品缺陷证据。
