# Why
输入诊断实际启用且存储受保护，但子Agent输入委派后root仍调用父Agent.record_input，消费父textarea delta；dump dispatcher直接取ActiveView::Agent根Agent，忽略子Agent。Dashboard attached popup能处理该chord，却因顶层不是Agent在dump时直接返回。

# What Changes
核对并统一记录与导出的目标归属：诊断应包含实际接收输入的composer/session数据，覆盖嵌套子视图和dashboard附着会话；保持200条上限与字符脱敏。

# Impact
仅诊断记录/选择，不改输入业务动作、发送或上下文路由；不删除真实DumpInputLog chord。修复前需确定父级是否保留聚合记录，不能单独改dump目标而让子日志一直为空。
