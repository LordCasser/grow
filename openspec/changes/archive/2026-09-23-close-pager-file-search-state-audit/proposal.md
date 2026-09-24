## Why

审计怀疑文件搜索关闭时保留的 selection、hover 和 scroll 会污染后续查询。调用链证明交互状态会在重开时重置，但也发现 daemon tick generation 不能识别尚未处理的新 query；本 change 修复迟到结果泄漏并固化交互状态证据。

## What Changes

- 为异步文件搜索结果增加查询请求身份，拒绝关闭后重开或输入新查询期间迟到的旧结果。
- 增加交互状态重置和旧请求结果拒绝的回归测试。
- 收窄 Pager backlog 中关于 `clear_context` 的怀疑，保留独立 worker 生命周期审计。

## Capabilities

### New Capabilities

### Modified Capabilities

- `client-surfaces`：文件搜索只展示和允许选择当前查询的结果。

## Impact

- `crates/codegen/pager/src/views/file_search/state.rs` 的单元测试。
- `openspec/backlog.md` 的 Pager 搜索审计条目。
