# 验证与对抗性审查

## 行为核对

上一版将 /btw 作为图片转换发起者，已按用户明确的 owner 边界更正：main 是唯一视觉辅助/OCR/ImageProjection 发起者；/btw 等待已启动主转换及 durable ACK，读取同一已提交 Surface 的图片描述分支。配对图片保持 immutable 原图，描述只在请求副本上选取。/btw 不再写能力、不恢复图片拒绝；仅保留原有 overload-only 重试。

准备后的 client/model/backend 仍冻结，文字选择不依赖负能力缓存。图片来源来自同次 materialize 的 ConversationItem，source ref 与主转换已提交内容一致。读锁在 /btw 创建 Sideband/request 前释放；主 write 锁在 durable ACK/失败之后、结果通知之前释放，跨 SurfaceChanged 重试仍由主转换持有。

## 测试

- `RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 cargo test --locked -p shell --lib side_question -- --test-threads=2`：10 passed，0 failed。覆盖 Messages/Chat Completions/Responses × user/tool images 的主转换描述复用（特意不写 negative cache），先完成主投影后两次 /btw 无额外 auxiliary，主上下文前后完整值一致。
- `side_question_waits_for_primary_conversion_and_durable_ack` 阻塞主 auxiliary attempt ACK，再阻塞 ImageProjection ACK，两段分别确认 /btw pending 且无提前请求。然后阻塞 /btw 自己的 attempt ACK，验证主转换能取得 write lock，证明 SideQuestion 请求不会持锁阻塞主会话。
- `side_question_does_not_start_missing_primary_conversion_or_recover_rejections` 验证 known-text missing 时零 HTTP、未知媒体 provider 明确拒绝后没有 recovery/negative write，主上下文完全不变。
- 等待 session cancellation 的测试确认任务返回且没有 HTTP；主投影永久失败的测试确认 /btw 不发出独立请求。
- 同次最终实现产物 `target/debug/deps/shell-2fc8b6c79ad17eb3`，在 shell crate cwd 设置 RUST_MIN_STACK=16777216 执行 `session::actor::tests::image_input_recovery_tests --skip side_question --test-threads=2`：18 passed，0 failed，覆盖原视觉辅助、OCR、持久恢复、混合移除、取消及 ACK 等待。
- `git diff --check`、全量 OpenSpec strict 和 archive validation 均通过；归档后重跑。

## 审查

独立只读审查确认没有锁环：主恢复不调用 /btw；read guard 只覆盖冻结及描述选择，provider/backoff 不持锁；Tokio RwLock 的 writer fairness 避免新 SideQuestion 持续插队。描述从同一冻结 item 的一致配对描述选择，不串用其他来源。失败/取消按 RAII 释放锁。

未将尚未启动的未来转换交给 /btw 调度：known-text 且没有主转换 owner 的未描述图片返回 context-not-ready；未知/native 模型的未转换媒体保留此前乐观语义。原有接收端关闭与后台任务 owner 的生命周期债务仅更新 backlog，不扩大本次范围。

## 构建范围

仅本地 debug 测试；未替换已安装 grow。沿用共享 Cargo target，未新增独立 target 目录。已有 atomic fetch_update 弃用及 macOS unwind linker 警告未混入修复。
