## Reproduction
临时目录创建真实目录与符号链接。旧实现下添加真实路径后 ignore 未清空；符号链接来源计数为 0 而非 1。两条新增回归失败，原 11 条通过。

## Final result
`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib extensions::skills::tests --quiet -- --skip test_resolve_tilde_path`：13 passed。跳过旧 HOME 全局修改测试。链接存在既有 macOS unwind 警告。

新增符号链接回归在 Unix 执行，验证添加清理 ignore、保留原 paths 文本、不重复添加、按真实路径移除，以及计数。既有组件边界和相对请求解析测试仍通过。没有真实 ACP 写盘或跨平台符号链接 E2E；tilde 管理通过既有解析器接入，未单独新增环境修改测试。

## Limits
不存在路径回退、删除后链接的历史目标及 ${VAR} 形式路径仍未统一。库测试不重新链接 target/debug/grow。磁盘可用约 69 GiB，target 9.8 GiB，本轮未增长到需要清理的规模。
