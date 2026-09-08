## Reproduction
旧实现 15 passed、1 failed；默认 cwd 的唯一不存在路径返回 ./missing-skill-UUID，违反绝对路径断言。

## Final result
低磁盘配置运行 `cargo test --locked --offline -p shell --lib extensions::skills::tests --quiet -- --skip test_resolve_tilde_path`：16 passed。新回归验证默认与相对 cwd 均锚定，添加保存解析结果；既有路径、别名和变量管理回归通过。未创建缺失目标或修改进程 cwd。跳过旧 HOME 修改测试，仍有既有 macOS linker unwind 警告。

## Limits
未覆盖进程 cwd 已被删除而不可读取的情况，此时沿用旧回退。没有推断已删除 symlink 历史身份，也不进行可能改变 symlink 语义的 .. 词法折叠。本轮不包含真实 ACP 写盘；target/debug/grow 未重新链接。磁盘可用约 69 GiB，target 9.8 GiB。
