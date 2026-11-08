## Implementation

- [x] 对照当前源码与归档验证，修正冗余单次 loop 和陈旧文档。
- [x] 运行格式与 `cargo clippy --locked -p shell --lib --tests --no-deps`，确认退出 0、never_loop 消失并保留 warning 记录。
- [ ] 使用最终 CLI 执行真实 ACP prompt admission replay 回归；编译/Clippy 不能替代运行证据。
- [x] 验证 OpenSpec 与相对链接，记录剩余证据边界：本工作树 strict 23/23、链接及 git diff --check 通过。
