## Reproduction
临时 Git 仓库测试旧实现失败：结果包含 sub/.grow 与外部祖先 .grow，缺少 repo/.grow。

## Final validation
低磁盘配置 `cargo test --locked --offline -p agent --lib prompt::skills::tests --quiet`：95 passed。新增回归验证 cwd 别名仓库边界及 .grow 链接保持 Local。

首轮全模块 94 passed、1 failed：已有 ignore 后仓库同名回退测试按 /var 字符串前缀比较 /private/var 的正确文件。改为规范文件路径相等，新增 Repo scope 断言，保留回退语义并提高验证精度。

## Scope
collect_skill_config_dirs 和 list_skills_with_roots 规范 cwd/Git root；并未将 .grow 链接本身替换为共享目标。未知/不可 canonicalize 路径仍回退原值。Unix 符号链接回归在 macOS 运行，不代表 Windows E2E。CLI 尚未重新链接，target 9.8 GiB，可用约 69 GiB。
