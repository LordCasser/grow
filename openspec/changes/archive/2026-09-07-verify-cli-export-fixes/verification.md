## Results
低磁盘配置构建成功；既有 macOS compact unwind 警告仍存在。

- --version: exit 0; stdout 31 bytes; stderr 0 bytes
- grow 2.1.4 (1e1fda6d) [stable]
- --help: exit 0; stdout 6129 bytes; stderr 0 bytes
- export --help: exit 0; stdout 533 bytes; stderr 0 bytes
- path: /Users/lordcasser/workspace/projects/grow/target/debug/grow
- size: 459086080
- sha256: 6a541e810b59a9894129ac5e94085a5bbb7e2dc26ba428bd806d908ee2fd98e9

## Included changes
fix-cli-export-clipboard-result、fix-session-export-relative-path、fix-clipboard-character-stats、fix-atomic-transcript-export、fix-export-during-history-load。

## Limits
只验证链接和三个入口，未导出真实会话或操作剪贴板。未替换用户安装版本。Git 哈希不唯一代表未提交构建，用 SHA-256 区分。

构建后 target 11 GiB，可用 66 GiB。本轮未清理仍会复用的构建缓存。
