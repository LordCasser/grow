## Why

ChatState 可能已把模型响应提交到 canonical Timeline，但 Shell 在收到 command reply 前丢失确认。当前 Shell 将该状态压成普通 turn error；配置 completion requirement 时会启动新的模型 Step，既无法核对已提交响应，也违反“确认不明时不得盲目重新采样”的现有契约。

## What Changes

- 在 assistant response 的 canonical `MessageEvent` 上持久化原 sampler request 与最终 attempt 组成的 admission identity，使冷恢复可以把响应关联回原提交。
- response admission 按 identity 幂等：完全相同的本地重放返回原结果，不追加第二份响应；同 identity 不同 payload 冲突并 fail closed。
- Shell 将原 identity 和原 payload 绑定到当前 Timeline admission；当前 owner 的确认不明时直接返回类型化 turn-boundary failure，禁止 completion recovery、新 provider 请求、Accepted 发布和工具执行。cold/replacement actor owner 可用后，exact identity/payload reissue 由 Timeline 幂等核对。
- 历史无 identity 的 response 继续作为既有历史读取，但不能用于确认新的不明 admission；provider-native continuation 仍是瞬态状态，不从历史响应恢复或被 duplicate admission 覆盖。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `session-timeline`：明确 response admission identity、幂等核对、冲突和确认仍不明时的停止语义。

## Impact

主要影响 `crates/codegen/chat-state/src/{timeline.rs,commands.rs,handle.rs,actor/}` 的 durable response admission，以及 `crates/codegen/shell/src/session/actor/turn/mod.rs` 的接纳错误策略和测试。无需新增 actor、持久化 sidecar、外部依赖或 provider retry 机制；`client-surfaces` 既有 Accepted/tool gate 不改变。
