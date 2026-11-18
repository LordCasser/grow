# Verification

- 从报告会话的只读采样记录核对了三次失败。HTTP 状态均为 200。三次都在 `response.content_part.done` 保存了缺少 `annotations` 的非空文本，其中一次还保存了同样缺字段的 `response.output_item.done`。同一会话另一个已捕获的辅助采样包含缺少该字段的 `response.completed`。记录中未改动会话正文或原始响应。
- 用无会话内容的 loopback SSE 夹具复现这些帧形状。固定 Rust 1.93.1 下，`cargo test --locked -p sampler --lib responses_text_snapshots_without_annotations_complete_and_keep_raw_evidence` 在仅加夹具时因 `missing field annotations` 失败。修复后同一夹具跑通 HTTP、SSE、SDK 解码及响应组装，正文及用量正常，省略的 cache read 仍为未知，captured evidence 与 HTTP 原始字节相同。
- 定向回归也覆盖无终态的已关闭文本项、显式 null、已有引用、同消息多段文本、重复的已知 ID 与未识别的保留字段。既有的初始文本、函数调用及终态用量严格性检查继续运行。
- `cargo test --locked -p sampler --lib -- --test-threads=4` 最终 296 项通过。一次默认并行运行有一个认证日志测试失败，单独复跑与随后两次 4 线程全量均通过。`cargo check --locked -p cli --bin grow` 和全仓格式检查通过。
