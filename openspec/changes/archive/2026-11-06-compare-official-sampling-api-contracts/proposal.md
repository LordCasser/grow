# Compare official sampling API contracts

## Why

用户要求从相关API官网与SDK收集依据，重新分析同类序列化问题。上一轮基于Grow源码及本地实验确认了usage补全失真、错误预解析边界，但官方wire契约、官方SDK运行时行为和兼容代理现场形状尚未逐项对照。

## What Changes

采集并固定OpenAI Responses/Chat的官方OpenAPI、Python/TypeScript SDK和官网说明，Anthropic Messages官网与官方SDK，以及DeepSeek官方Chat兼容说明。用户随后明确重点为同一会话跨协议、跨模型切换，因此先核源响应持久化、route/native状态隔离、工具历史目标投影、reasoning回传与目标接纳，再以初始空前缀、required/nullable、失败事件、usage和错误识别作为关联证据。记录官方保证、SDK容错与Grow实现的差异，必要时收窄或纠正上一轮结论。

skip_specs:true：本次仅外部资料研究与文档修订，无生产行为、接口、状态、依赖或持久化变化，不虚构delta。分析以现有main8ab486a6和锁定async-openai为本地基线；公开2.6.1tag与资产、用户运行会话/配置不变，不调用真实provider或发布新版本。
