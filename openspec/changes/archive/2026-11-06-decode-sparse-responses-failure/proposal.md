# Preserve sparse Responses provider failures

## Why

用户指定会话01a1a9c0-03b9-76d0-b513-923efdcfd80a的两个子任务报missing created_at。父复核原流发现：created/in_progress均有有效created_at和同一response ID，服务在输出中途最后发送仅含error/object/status的response.failed，错误为server_error / connection lost mid-reply。SDK要求完整Response和sequence_number，先报本地Serialization，遮住了真实服务失败并错误进入FatalSerialization路径。

这项修复归2.6.1之后版本；2.6.1已发布及验收，tag/source和资产不变。不填now/0或伪造完整response，不执行原会话任务。

## What Changes

Responses client在常规typed decode失败时识别已证实的精确稀疏失败包：顶层仅type/response，内层仅error/object/status，error仅code/message，明确response.failed / response / failed和非空字符串错误。复用既有SamplingError::from_stream_error传播provider错误及既有有限恢复策略，记录真实failed诊断，不产出Completed或工具接纳。

## Impact

仅Sampler client解码边界及同文件真实HTTP/SSE回归、开发说明与本契约。完整typed ResponseFailed路径保留usage/终态处理；其它已知malformed形状继续拒收。无新公共类型、依赖、状态、默认时间或重试循环；不发布另一个版本。
