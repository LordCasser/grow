## Why

自更新执行 `--version` 校验时，超时只丢弃等待 future；Command 未设置 kill_on_drop，已启动进程仍可能继续运行。目标版本探测和新二进制冒烟检查都有同一缺口。

## What Changes

- 给两类短期版本校验 Command 设置随 owner 释放终止直接子进程。
- 增加真实 Unix 子进程的超时/取消与正常返回回归。
- 补充 client-surfaces 的自更新校验进程生命周期契约及开发者说明。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `client-surfaces`：明确自更新版本校验进程在等待超时或取消时必须终止。

## Impact

`crates/codegen/update/src/auto_update.rs` 的 `probe_version_by_exec` 与 `smoke_test_binary`。不改变 detached 后台更新子命令，不改下载、发布事务、超时长度和 ETXTBSY 重试，不新增进程管理框架。
