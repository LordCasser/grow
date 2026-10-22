# Verification

## Incident evidence

- Session：`01a15c48-ab5b-7872-a263-15c28b8d8e69`。
- `~/.grow/logs/unified.jsonl` 原始行 15376–15380：2026-10-21 17:44:24（Asia/Shanghai）prompt `f3c0763d-bcf3-4e72-8978-21e0f7f13bd6` checkpoint 在 1ms 内失败，`input intent was not captured`；没有同 ID 的 ready.execute 或 acp_send。17:45:32 重发被 `turn_running` 阻塞。
- `~/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fvulnerability%2Fphp2shell/01a15c48-ab5b-7872-a263-15c28b8d8e69/timeline.jsonl` 没有该 prompt identity。未修改真实 session，也未输出图片二进制或凭据。
- 代码证据：queue drain 先设置 TurnSubmitting/current_prompt_id；local_drafts 拒绝保存带图片的 uncertain submission；旧 checkpoint 错误分支恢复草稿但不释放占位。新分支仅释放匹配 identity 的 submitting 状态。

## Validation

- OpenSpec 归档前全量严格校验：21 passed，0 failed。
- 归档后全量严格校验：20 passed，0 failed；archive 校验：675 passed，0 failed。
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p pager --lib image_checkpoint_failure -- --test-threads=1`：3 passed，0 failed。覆盖生产图片 checkpoint 拒绝、显式原样重试、成功 gate、草稿编辑、其他/运行前台隔离。
- 同一编译产物 `target/debug/deps/pager-a429a183957e3f63` 执行完整 `app::root::dispatch::tests::turn_pipeline::`：10 passed，0 failed；精确执行 `local_drafts::tests::ram_only_input_retry_does_not_loop_through_the_failed_writer`：1 passed。首次直接调用使用了错误相对路径（exit 127），改绝对路径后完成，不把未运行视为通过。
- `rustfmt --edition 2024 --check`（两个修改的 Rust 文件）、`git diff --check` 通过。编译存在其他代码的弃用/noop clone 和 linker unwind 大小警告，没有本次编译错误。
- 复用现有 debug target，未创建独立构建目录；构建期间磁盘约 1.2 TiB 可用。工作树还有其他并行 release 编译，不清理其共享产物。

## Adversarial review

- 本地 checkpoint gate 失败意味着本次 send 未执行，不能推广到 RPC 错误的接纳未知路径。
- 仅释放 state=TurnSubmitting 且 current_prompt_id 相同的占位；已运行或其他 prompt 保持身份与计时。
- 草稿恢复继续遵守新编辑保留与 requires_review；失败不返回发送 effect。
- 原样显式重试沿用原身份与内存图片，通过现有 RAM-only gate；不扩展本地附件持久化。
- 未执行真实 provider 图片调用；修复边界在发送前，模型能力与上传不受本次验证覆盖。
