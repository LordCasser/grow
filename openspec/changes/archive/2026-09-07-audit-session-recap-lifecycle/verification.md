# Session recap 审计

本轮未修改产品代码；以下区分源码事实与已运行测试，不声称完整生命周期已验证。

## 入口与边界
- `shell/src/agent/config.rs::resolve_session_recap` 默认开启；`extensions/recap.rs::handle` 同时限制手动和自动入口，等待 session load 后排队。它忽略 send 返回值；已关闭 channel 的应答语义需继续核对。
- `session/actor/run_loop.rs` 为 Recap 创建 session activity，并 spawn_local 执行；不是前台用户 turn。
- `session/actor/recap.rs::handle_recap` 在首次 await 前捕获 recap_epoch，再 materialize Timeline。自动要求至少 3 个主 turn、有新 turn、距上次 API 请求至少 3 分钟；手动无 turn 也拒绝。
- recap_in_flight 排除重叠生成；当前通过各返回分支手动清理。没有据此推断所有 future 丢弃路径都安全，需先确认生产取消路径。
- 请求无工具；Sideband 记录冻结来源、尝试、usage 和结果，summary 只用于展示。最终 epoch 检查与 watermark 提交在无 await 段执行。
- 新用户输入通过 epoch 使过时 recap 不展示；不是立即取消 provider 请求，旧 Sideband 可以完成。自动超长结果保留 Sideband 而抑制展示。

路径均位于 `crates/codegen/`。

## 测试证据的含义
`recap_display_only_tests` 包含空输入/失败/预算裁剪/epoch 提交/旁路记录断言；`recap_request_rides_parent_prompt_cache` 用本地 MockInferenceServer 确认 Responses 请求包含父 prompt_cache_key 且没有 tools/tool_choice。部分名称与注释提及 tools，但实际断言明确禁止工具。
单独 `manual_recap_never_mutates_conversation` 比较前后 surface，不能仅凭此断言成功 provider 返回、全部取消窗口或模型切换一致性。

## 下一项可执行核对
`prepare_chat_completion` 经 refresh/reconstruct 冻结 client；handle_recap 随后另一次读取 sampling_config 取得 model/window。需核对两次读取间模型切换是否能让 route/backend 与 request model 不一致，并以受控交错验证后决定修复。此处只是候选窗口，尚非已复现 bug。

Hook stdin 快速复核：runner 并发写入/读取，stdin write_all 错误刻意忽略，再按退出状态/输出判断；未发现早退 BrokenPipe 必然误报的证据。

## 实际运行结果
`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib recap --quiet`：55 passed、0 failed/ignored，运行 0.14 秒。已有 macOS 大体积 unwind table linker warning，未阻止测试。没有请求真实 provider 或读取用户会话。
