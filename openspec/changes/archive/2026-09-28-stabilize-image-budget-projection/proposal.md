## Why

`build_conversation_request` 每轮从完整 Surface 克隆请求，图片请求达到 47 MiB 后在副本中回收到 25 MiB。下一轮又从原图重建，所以持续追加图片会逐轮移动淘汰边界；现有“批量回收后长期稳定”的注释和同一 slice 幂等测试没有覆盖真实跨轮行为。投影变化还会重置 native continuation epoch，进而改变 `prompt_cache_key`。

## What Changes

- 在当前 ChatStateActor 内保留图片预算选择，使相同历史图片在后续纯追加请求中继续使用相同 placeholder；按应用既有选择后的大小决定是否再次回收。
- 只有重新达到高水位才推进淘汰边界，回收到既有低水位；支持 User 与 ToolResult 中的多图片 parts。
- 明确该临时选择在 Surface 替换、路由替换、描述投影切换、fork 和冷恢复时的生命周期；原始 Timeline 图片证据继续保留。
- 用跨多次 `build_request` 的反例覆盖预算、native epoch 和 key 的联动，修正超出测试证据的缓存命中注释。

## Capabilities

### Modified Capabilities

- `model-sampling`: 新增跨请求图片预算选择的稳定性与失效边界。

## Impact

实现集中在 `crates/codegen/chat-state/src/actor/request_builder.rs`、`state.rs` 及相关 actor 控制入口；验证三种 backend 的图片附件编码和 sampler 最终 wire body 上限。开发者说明在实现时更新 `docs/development.md` 并链接本 change 归档后的规范。

不调整 47/25 MiB 阈值、token compaction、图片正规化、provider 参数、cache key 公式或 native replay 合法性。无前置 change；可与 [缓存计量](../preserve-cache-usage-availability/proposal.md) 独立实施。后续 [provider 验证](../evaluate-provider-cache-affinity/proposal.md) 使用本项修复后的基线。
