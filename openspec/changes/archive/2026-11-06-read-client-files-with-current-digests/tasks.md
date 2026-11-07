# Tasks: read-client-files-with-current-digests

- [x] 1. 建 proposal、delta（WHEN/THEN）、design、tasks；`openspec validate read-client-files-with-current-digests --strict --no-interactive` 通过。
- [x] 2. 实现：删 `FileHashMemo`/`MemoEntry`/`HASH_MEMO_CAPACITY` 与 `WorkspaceShared::client_fs_hash_memo` 字段/构造；stat 与 read_file 均改走 `stream_hash_and_range`，`size` 用同遍 streamed 值，metadata 仅决定 kind/mtime 显示。
- [x] 3. 测试：公共 `WorkspaceOp::execute` 路径覆盖同 size/保持 mtime、同 ms 不同 ns、atomic replacement 三种改写（hash=新内容全量摘要、range/size 相符、mtime_ms 不变）；删 memo 专属测试；confinement/cap/binary/分页既有断言保留。
- [x] 4. 自文件 rustfmt 与最终 diff 自查（不跑 cargo，不 workspace fmt）。
- [x] 5. main 共享 target 运行本项测试验证（父授权编译窗口内执行）：client_fs 18/18 通过（含 3 个 WS-02 回归）、file_state 41/41 通过；envrc 7/8，失败项属并行 envrc 改动范围，已报父。证据见 [verification.md](verification.md)。
- [x] 6. 父核对生产 diff、公共入口的 18 项实测结果及场景映射，更新开发者说明；envrc 的独立失败另项处理，不作为本 ClientFS 项通过证据。
