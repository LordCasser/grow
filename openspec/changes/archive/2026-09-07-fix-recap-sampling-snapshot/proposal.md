## Why
handle_recap 在 prepare_chat_completion 冻结 endpoint/backend 后，另读 chat_state sampling_config 设置显式 model/context_window。SamplingClient 的 conversation_collect 接纳显式 request.model，配置变化可能使新模型发送到旧 endpoint。

## What Changes
recap 从单次准备的完整配置同时取得 client、model 和预算，消除第二次配置查询。

## Capabilities
### Modified Capabilities
- model-sampling: recap 采样路由与预算快照一致。

## Impact
Shell 采样准备 helper 和 recap。其他旁路调用者的类似窗口独立审计。
