## Results
低磁盘配置 cargo build --locked --offline -p cli --bin grow 成功。既有 macOS __eh_frame compact unwind 警告仍存在。

- --version: exit 0; stdout 31 bytes; stderr 0 bytes
- grow 2.1.4 (1e1fda6d) [stable]
- --help: exit 0; stdout 6129 bytes; stderr 0 bytes
- path: /Users/lordcasser/workspace/projects/grow/target/debug/grow
- size: 459079872
- sha256: 57156ef1fb973346804e257109f0474ac563b95e2425724b8b9deea357fed45d

## Limits
只验证当前工作区链接及早期命令，未执行真实会话或用户配置写盘。未替换 ~/.local/bin/grow。Git 版本哈希不唯一标识未提交构建，使用上述 SHA-256 区分产物。磁盘可用约 68 GiB，target 9.8 GiB。
