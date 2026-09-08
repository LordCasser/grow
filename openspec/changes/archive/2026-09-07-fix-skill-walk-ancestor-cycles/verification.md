## Reproduction
临时目录只有根和子目录两个技能，子目录 loop 指回根。旧实现返回 7 条递归别名路径，新回归失败。

## Final validation
低磁盘配置分别执行 `cargo test --locked --offline -p tools --lib implementations::skills --quiet`（98 passed）与 `cargo test --locked --offline -p agent --lib prompt::skills::tests --quiet`（95 passed）。新回归验证祖先环不再收集重复文件，也验证两个独立别名仍按原顺序返回。

## Scope
祖先集合进入子目录前插入、离开后移除，检查先于 SKILL.md 收集。保留深度上限和原词典序，不做全局目录去重，也不改变合法链接加载。Unix 临时链接测试不等于 Windows E2E。未运行真实慢文件系统压力测试；CLI 尚未重新链接。磁盘可用约 69 GiB，target 9.8 GiB。
