## Regression
旧实现下，添加 foo 后 ignore 中 foobar 被删除（剩余 /other）；来源计数返回 3 而非 2，两条回归均失败。首次测试因新增 tests 模块与既有模块重名而编译失败，合并后才获得上述行为复现。

## Final validation
使用低磁盘配置运行 `cargo test --locked --offline -p shell --lib extensions::skills::tests --quiet -- --skip test_resolve_tilde_path`，最终 11 passed。跳过旧的进程 HOME 修改测试；其余 9 条既有测试和 2 条新增回归通过。仍有既有 macOS linker unwind 警告。

生产添加闭包调用被测 add_skill_path；添加响应、自动来源、自定义路径展示均调用被测 count_skills_from。测试不包含完整 ACP 写盘会话。此前生成的 target/debug/grow 未因 lib test 更新，不包含本轮修改。

## Deferred
reset 的注入者证据仍缺失，保留原债务；路径别名语义独立登记 backlog。本轮无删除候选变更。
