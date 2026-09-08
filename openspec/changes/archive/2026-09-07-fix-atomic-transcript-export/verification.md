## Results
cargo test --locked --offline -p pager --lib export_ --quiet：4 passed，0 failed。无增量、debug=0、2 jobs、RUST_MIN_STACK=16777216；仅有既有 macOS compact unwind 警告。

- 注入临时文件部分 write_all 成功后返回错误：旧文件仍为 old export，目录仅剩目标文件；随后正常替换内容正确且无临时文件残留。
- 新嵌套目标创建成功，目录目标拒绝。
- Unix 新文件权限 0600；已有 0640 保留；现存相对 symlink 保留且实际目标更新；悬空链接拒绝并保留；readonly 目标拒绝且内容不变。
- 前一轮 session cwd/绝对目标真实 dispatcher 回归继续通过，CLI 剪贴板结果回归亦通过。
- 源码核对 CLI 与 TUI 均调用 write_export_file，不再直接 std::fs::write 导出正文。

## Limits
故障通过临时写入闭包注入，不是填满磁盘或断电测试；未测试 Windows 文件系统或平台扩展 ACL。父目录未 fsync，不声称掉电后目录项持久化。同步 I/O 的响应性仍待处理。未重新链接 CLI 或替换用户安装程序。
