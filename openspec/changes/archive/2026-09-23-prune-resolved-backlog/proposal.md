# Change: Keep backlog focused on open work

## Why

`openspec/backlog.md` 同时包含待处理债务和大量已经归档的修复记录。已完成事项的权威契约与验证已在 `openspec/specs/`、`openspec/changes/archive/`，重复留在 backlog 使未完成事项难以定位，也容易把历史限制误读为当前问题。

## What Changes

- 逐项核对标记已完成或已有归档证据的条目，仅移除没有残余未完成问题的记录。
- 混合条目保留真正未完成的边界和必要归档链接；对已经立项的事项明确其 active change 状态。
- 记录每组移除、保留与不确定项的证据，不凭名称或旧测试结果推断问题已解决。

## Impact

仅维护 `openspec/backlog.md` 和本 change 的审计记录。不改产品行为或主规范，故设置 `skip_specs: true`。并行 active changes 分别按其原提案收尾，不在本次文档清理里混入实现。
