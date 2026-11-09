## 本地候选

- 核对 v2.6.2 和 origin/main 的六个后续提交，确认 PR #3、#4 均在基线历史中；子 Agent 修复无冲突重放。
- 使用 Rust 1.93.1 执行锁定的 Cargo metadata，确认 CLI 和 33 个继承 workspace 版本的本地包均为 2.6.3。完整依赖解析限定本机 aarch64-apple-darwin，平台构建由发布门禁执行。
- 运行 git diff --check 与严格 OpenSpec 验证。正式发布使用 release.yml 的注解 tag、源码身份检查、同提交回归、跨平台构建与完整资产校验。
