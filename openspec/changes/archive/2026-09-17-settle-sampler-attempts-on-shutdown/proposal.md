## Why

正常 session teardown 会先取消 sampler 请求，再由 `SamplerActor` 调用会 abort 剩余任务的 `JoinSet::shutdown()`。若请求正等待 attempt evidence 或用量账本 ACK，teardown 可能在结算闭合前返回，违反每次已准入 provider attempt 必须保留证据并完成适用账本结算的现有契约。

## What Changes

- sampler 正常关闭时先停止新请求、取消 provider 工作，再协作等待所有已准入 request task 完成 evidence 与 usage settlement；正常关闭路径不再直接 abort request task。
- 保留现有有界强制关闭：只有 owner 的明确 shutdown deadline 到期后才允许 abort，并向 session teardown 返回失败，不跨越成功的最终持久化边界。
- 增加阻塞 evidence ACK、阻塞 usage ACK 与强制超时的回归，证明正常关闭等待结算、强制关闭不冒充成功。
- 不改变普通请求的重试、接纳、用量身份、超时时长或公开 sampler API。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `model-sampling`：明确 sampler 关闭时已准入 attempt 的取消、证据与用量结算边界，以及强制超时的失败语义。

## Impact

主要修改 `crates/codegen/sampler/src/actor/mod.rs` 及其测试；`crates/codegen/shell/src/session/actor/teardown.rs` 继续持有十秒强制关闭与 event drainer 顺序，不新增依赖、持久化格式或公共类型。开发者说明同步记录 graceful/forced shutdown 的区别。
