## Results
构建成功；既有 macOS compact unwind 链接警告仍存在。

- --version: exit 0; stdout 31 bytes; stderr 0 bytes
- grow 2.1.4 (1e1fda6d) [stable]
- --help: exit 0; stdout 6129 bytes; stderr 0 bytes
- export --help: exit 0; stdout 533 bytes; stderr 0 bytes
- path: /Users/lordcasser/workspace/projects/grow/target/debug/grow
- size: 459095200
- sha256: a64ab21ea3ceafed6c77305ba16cb93ee1f4fdc1bebe4fac5086f91ee21755b8

## Included changes
fix-session-copy-relative-path、fix-atomic-copy-file、fix-copy-message-selection、fix-copy-delivery-notices、fix-copy-invalid-numeric-args。audit-restore-degree-cache 只登记 R12，未删除。

## Limits
未通过 CLI 连接真实会话或调用剪贴板，入口 smoke 不替代模块回归。未替换已安装 grow，版本 Git 哈希不能唯一代表未提交构建，以 SHA-256 区分。

构建后 target 12 GiB，可用 65 GiB；未在本轮清理仍在复用的缓存。
