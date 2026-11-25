## Why

同一工作目录的两个并发 Grow 会话通过 Messages 调用有状态网关。请求使用相同的系统指令和首条运行环境消息，未发送区分会话的传输标识。网关将两者交给同一个上游对话，多次收回未完成的工具交换并按有限历史重新建立上下文。

## What Changes

普通采样以现有不透明分支路由键发送会话亲和头，独立 Sideband 操作使用自己的标识。已有显式请求头配置优先。

## Impact

涉及 Sampler 的请求传输、Shell 的 Sideband 采样边界及 model-sampling 规范。
