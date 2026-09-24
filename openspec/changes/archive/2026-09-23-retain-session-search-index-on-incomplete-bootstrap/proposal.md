## Why

搜索索引 bootstrap 会把损坏的会话摘要当作可忽略条目，并把单会话 Timeline 或索引写入失败计入“已完成”。随后 orphan prune 会删除旧索引行，完成标记又阻止同一进程的 Recheck 在文件修复后补回数据。

## What Changes

- 搜索专用枚举将已打开会话目录中的缺失或无效摘要报告为扫描失败，普通会话列表与 TTL cleanup 的无效候选处理保持不变。
- bootstrap 只有在所有必需 Timeline 读取和索引写入成功后才会 prune orphan 并发布完成标记；失败保留既有行并允许 Recheck 重试。
- 覆盖真实临时存储根下的摘要损坏、Timeline 失败、修复及 Recheck 恢复路径。

## Capabilities

### New Capabilities

### Modified Capabilities
- `client-surfaces`: 明确不完整的搜索索引 bootstrap 不得丢弃现有索引行或发布完成状态。

## Impact

- 影响 `crates/codegen/shell/src/session/storage/search.rs`、`search_fts.rs`、`jsonl/mod.rs` 和对应存储测试。
- 更新 `openspec/specs/client-surfaces/spec.md` 的搜索 bootstrap 契约，并在架构修复说明中记录严格搜索枚举与普通列表/cleanup 的边界。
- 不改变 cleanup Once 的产品语义，不增加依赖或索引 schema。
