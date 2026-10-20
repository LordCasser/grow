# Design

以现有 Runtime Checkpoint 为跨执行周期等待的权威。在折叠 ExecutionReleased 时，仅让匹配 operation checkpoint 中 AwaitPermission/AwaitUser 且 kind 一致的现存交互证据跨越边界；不复制任意旧 cursor、不保留所有历史 Suspended。新 Opened 覆盖同 call 的旧 attempt；真实 Resolved 精确移除对应 attempt；调用已有 ToolResult 时仍由现有分类前过滤剔除。Checkpoint 不新建 attempt，第二次挂起不重复关闭已 Suspended 的旧 attempt。

验证三类等待：第一轮 drain 超时→重试→冷恢复→release 后阻塞 worker 调度→第二次挂起。检查未重绑、新 attempt 接管、真实解决与工具结果优先、旧 checkpoint 未保留等待均不被复活。
