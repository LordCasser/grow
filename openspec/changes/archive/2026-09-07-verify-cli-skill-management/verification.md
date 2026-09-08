## Results
低磁盘配置 cargo build --locked --offline -p cli --bin grow 成功。macOS linker 仍提示既有 __eh_frame compact unwind 警告。

- --version: exit 0, stdout 31 bytes, stderr 0 bytes
- grow 2.1.4 (1e1fda6d) [stable]
- --help: exit 0, stdout 6129 bytes, stderr 0 bytes
- binary: /Users/lordcasser/workspace/projects/grow/target/debug/grow
- size: 459102720
- mtime: 2026-09-07T20:03:56.925789
- sha256: a125c0f835be99cd0042a9eafcc8a15f5f9e4184ded2dfd9e036f850bb25b8d2

## Limits
包含当前工作区技能管理修复；只验证链接与早期命令，不代表真实交互会话或 ACP 写盘测试。未替换 ~/.local/bin/grow。版本哈希不包含未提交内容，以上产物 SHA-256 可用于区分具体二进制。

磁盘：target 9.8 GiB，可用约 68 GiB；保留有效缓存，无并发清理。
