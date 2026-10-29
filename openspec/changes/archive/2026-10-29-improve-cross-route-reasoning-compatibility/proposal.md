## Why
跨 provider/model/API 格式切换后的 portable 请求可能缺少目标要求的 reasoning。现有 HTTP 错误先截断展示文案再分类，长代理前缀可掩盖明确要求；Responses 恢复仅回放完整工具往返前的 reasoning，忽略普通回答前已有文本。既有模拟测试尚不能证明实际目标路由接受续跑。

## What Changes
- 在 HTTP/SSE 错误入口从未截断的结构化证据提取既有 typed reasoning 要求，展示继续保持短文案；明确字段缺失要求可以恢复，未知/冲突/不支持字段不得误判。
- 显式学习的 Responses 路由可回放紧邻普通 assistant 的已有非空 visible reasoning，维持工具配对、原历史和 opaque/native 隔离。
- 复用现有路由 ACK、logical sampling 预算和真实 Grow 会话切换完成跨 provider/model/API 验证，记录脱敏证据。

## Impact
涉及 sampling-types、sampler、chat-state 定向测试及开发者说明。无新的 provider registry、配置开关或历史存储格式。不会补造缺失的 Responses reasoning、签名或加密数据，不自动改变 thinking 配置或重新执行历史工具。宽泛 native 400 reset、reasoning provenance 与能力缓存的进一步改造单独记录，不混入第一阶段。
