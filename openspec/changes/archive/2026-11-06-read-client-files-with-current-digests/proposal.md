# Read client files with current digests

## Why

审计项 WS-02（`openspec/changes/archive/2026-11-05-coordinate-audit-backlog-repairs/design.md`）：`client_fs` 的 `FileHashMemo` 以 `(path, size, mtime_ms)` 复用全内容 SHA-256。size 相同、mtime 停留在同一毫秒的原地改写（含恢复 mtime 的写与 atomic replacement）会使 stat 返回旧摘要；read_file 命中 memo 时另开一次只读 range 的文件读取，可把新字节配上旧 hash 与旧 size。任何元数据元组都不能证明内容代际，父已定案移除该 memo：stat/read_file 都消费 `handle::stream_hash_and_range` 的同遍流式读取，hash、range 与上报 size 来自同一次打开的当前内容；元数据只决定 kind 与 mtime 显示。用户已授权全部非延期 backlog 项。

## What Changes

- 删除 `FileHashMemo`、`MemoEntry`、`HASH_MEMO_CAPACITY` 及 `WorkspaceShared::client_fs_hash_memo` 字段与构造；删除 memo 专属测试。
- `stat`（文件）与 `read_file` 均调用 `handle::stream_hash_and_range`：单次打开、单遍读取同时产出全内容 SHA-256、请求 range 与实际 streamed size；`size` 字段改用该遍 streamed 值，不再取预读取的 metadata `len()`。元数据仅用于 stat 的 mtime 显示与 read_file 的目录判定。
- 代价：重复 stat 会重复全文件 hash I/O，这是正确性的必要成本；不加 watcher generation、缓存实体或更细 mtime/inode guard。
- 并发原地 writer 下不提供文件系统原子快照，本项不宣称该保证；root-confinement、symlink 排除、read cap、binary-safe 编码、分页行为全部保持。

## Impact

- 代码：`crates/codegen/workspace/src/file_system/client_fs.rs`（实现与同文件测试）、`src/session/mod.rs` 与 `src/handle.rs` 仅删 memo 字段所需最小改动。
- 契约：`openspec/specs/client-surfaces/spec.md` 新增 client_fs 文件操作摘要/size 同源 requirement（本 change delta）。
- 不改 `ext_fs`、`get_files`、`session/file_state.rs`、`envrc.rs` 及其他共享模块。验证：公共 `WorkspaceOp::execute` 路径的同 size/保持 mtime、同 ms 不同 ns、atomic replacement 回归；cargo 测试由父在 main 共享 target 统一运行，本 worktree 不建 target。
