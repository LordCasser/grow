## Why

会话 `01a18aab-acf5-7fb2-a681-38402e52f0cc` 切换路由后上下文计数跳升并压缩。需要区分实际增加的 reasoning、token heuristic 误差与输出预留，不能凭旧路由用量判断新请求是否超窗。

## What Changes

- 从原 sampling evidence 重建被本地压缩门禁拦住的 reasoning 回放输入，核对原端点的实际计数/拒绝证据。
- 保留脱敏数字、重建方法、来源哈希与结论；不保存会话正文、reasoning 正文或凭据。
- 在同一 turn 的闭合 Step 边界切换路由后，为获准的下一 Step 持久化既有 synthetic AutoContinue，让新路由按历史事实接手任务，不把旧路由工具回合冒充本路由未结束的推理。
- 保留总窗口和显式输出预留检查，不跨路由继承校准、不改配置、不重写工具调用 ID。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `model-sampling`：跨路由的同 turn 续接边界。

## Impact

调查涉及 `sampling-types/src/conversation.rs`、`chat-state/src/actor/state.rs`、`chat-state/src/actor/mod.rs` 与 Shell 的恢复/压缩入口。原会话只读；诊断请求使用原 provider，绝不执行返回的工具调用。不改认证、模型配置或原会话。

实际变更仅在 Shell 已有下一 Step 准入处复用 AutoContinue。原失败请求无 usage；完整 reasoning 的受控 ID 配对替换变体为 693,053 input tokens，追加续接 user 边界变体为 693,060，均小于本地 1,370,640。693,053 加原输出预留 393,216 仍超过窗口 1,048,576；不能将修复承诺为永不压缩。
