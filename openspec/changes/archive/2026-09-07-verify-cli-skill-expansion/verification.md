## Results
构建成功；既有 macOS compact unwind 链接警告仍存在。

- --version: exit 0; stdout 31 bytes; stderr 0 bytes
- grow 2.1.4 (1e1fda6d) [stable]
- --help: exit 0; stdout 6129 bytes; stderr 0 bytes
- path: /Users/lordcasser/workspace/projects/grow/target/debug/grow
- size: 459083728
- sha256: 1ab059acc295c2bf49b3640b1ef71bc081e502872030058bef50ceeb8a1f236a

## Included changes
fix-plugin-skill-use-outcome、fix-skill-expansion-identity、audit-skill-slash-rewrite、fix-skill-envelope-attributes。

## Limits
只证明当前工作树 CLI 链接与启动入口可运行；未运行真实模型会话，未修改用户安装目录。版本中的 Git 哈希不能唯一标识未提交构建，以 SHA-256 区分。

构建后 target 9.8 GiB，可用空间 67 GiB，无需本轮 cargo clean。
