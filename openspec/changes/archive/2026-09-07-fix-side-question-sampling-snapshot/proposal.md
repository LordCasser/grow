## Why
/btw 在 client 准备后再次读取当前 model，存在旧 endpoint + 新 model 的混合快照。

## What Changes
复用 prepare_chat_completion_config，在构建 client 前从同一配置取得模型，所有重试维持该快照。

## Capabilities
### Modified Capabilities
- model-sampling: side question 路由快照一致。

## Impact
仅 /btw 准备逻辑和受控交错回归；不改变 AI Suggest/Prompt Suggest 的独立模型选择。
