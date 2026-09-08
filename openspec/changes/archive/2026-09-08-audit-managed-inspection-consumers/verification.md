## Findings
`config/src/managed_text/mod.rs`：ManagedTextInspection.original_text 是 Option<String>，plan 使用 original.bytes.as_ref().map(|_| text.to_owned()) 为已有源复制完整原文。公开 original_text() 的仓库内消费者只有 tests.rs::typed_inspection_and_item_updates_share_one_validated_parse 中一个断言。

`pager/src/diagnostics/fix.rs`：SSH 与 tmux 规划读取 unmanaged_text 检查定制；tmux 还读取 requested_item_state；planned_change_with_error 使用 requested_path、target_path、managed_block、backup_path_hint、changes_file 生成确认内容。未读取 inspection.original_text。

原始 SourceState.bytes 被事务备份、重验和回滚读取，必须保留。typed_inspection 测试还覆盖非托管正文、新旧条目及渲染结果，不能整项删除，只可移除专属断言和不再使用的局部原文读取。外部编辑器中同名 original_text 用于保护草稿，并非同一字段，不能按名称删除。

## Limits
全仓 Rust 引用搜索不证明仓库外公共 API 没有消费者；未测量实际内存节省。这里仅记录候选，不修改代码，无需重新编译。当前缓存152 MiB，可用76 GiB。
