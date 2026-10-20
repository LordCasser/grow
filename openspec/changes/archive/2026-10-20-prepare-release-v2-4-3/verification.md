# 发布准备验证

## 已完成的本地准备

子 Agent permission Settings 和四项 show_model_provider 清单修复的相关回归已通过：完整 Settings integration 231 passed、0 failed、0 ignored；Pager settings library 331 passed、0 failed、1 ignored；Pager 子 Agent permission 20 passed，Shell 相关 5 passed。组之间有交集，不累加；细节见 complete-settings-preference-test-inventory 验证。完整 Settings integration 已接入 Core regression，在正式发布时重新校验同一不可变 SHA。

workspace 2.4.2 → 2.4.3；Cargo.lock 只修改 33 个本地 workspace 包版本。按 TOML 对比将这些本地版本还原后，整份 lock 与 HEAD 完全相等，第三方依赖和 checksum 无变化。cargo metadata --locked --no-deps 确认 CLI 2.4.3。发行说明使用绝对链接。

Python scripts 34 项执行，33 通过、1 skipped；修改 Rust 文件 rustfmt --check、git diff --check 通过。OpenSpec strict 校验归档前 18 项 passed，0 failed。

## 发布步骤与边界

完成本地相关回归和归档后提交源码/测试/审计与版本准备；创建指向清洁 HEAD 的 annotated v2.4.3，运行 scripts/validate-release.sh，再原子推送 main/tag，从 main dispatch 官方 release.yml，全部平台启用且 publish=true。

准备归档只证明候选可供托管验证，不宣称公开发布已完成。跟踪全部门禁和十平台构建，通过后核对 Release 非草稿/非预发布/latest、十个 archive + SHA256SUMS、远端 tag commit、实际下载的 macOS ARM64 包摘要/签名/隔离配置版本身份，再追加最终结果。

归档后严格校验：Totals: 16 passed, 0 failed (16 items)；Totals: 663 passed, 0 failed (663 items)。
