## Reproduction
旧实现 14 passed、1 failed：使用当前 HOME 值和局部 SkillsConfig，添加真实目录后 `${HOME}` ignore 未被清除。普通请求保留变量字面文本的对照测试原本通过。无环境或用户文件修改。

## Final result
低磁盘配置 `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib extensions::skills::tests --quiet -- --skip test_resolve_tilde_path`：15 passed。保留原变量引用、不重复添加、移除和请求字面边界均通过。跳过旧的 HOME 修改测试；仍有既有 macOS linker unwind 警告。

## Scope
生产添加与移除的原始配置比较使用被测 helper；计数与普通请求不经过环境展开 helper。未执行真实 ACP 写盘或无 HOME 平台验证。target/debug/grow 未重新链接。本轮磁盘约 69 GiB 可用，target 9.8 GiB。
