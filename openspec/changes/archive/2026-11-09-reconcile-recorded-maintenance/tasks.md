## Implementation

- [x] 对照当前源码与归档验证，修正冗余单次 loop 和陈旧文档。
- [x] 运行格式与 `cargo clippy --locked -p shell --lib --tests --no-deps`，确认退出 0、never_loop 消失并保留 warning 记录。
- [x] 使用最终 CLI 执行真实 ACP prompt admission replay 回归：源码 ed2027c0 的实际 CLI 构建退出 0，精确测试 1 passed / 0 failed / 0 filtered out，真实运行 1.35s。
- [x] 验证 OpenSpec 与相对链接，记录剩余证据边界：归档前 strict 23/23，归档后 all strict 22/22、archived strict 925/925、链接及 git diff --check 均通过。
