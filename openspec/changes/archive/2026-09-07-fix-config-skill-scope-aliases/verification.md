## Reproduction
旧实现配置来源测试 8 passed、1 failed：外部符号链接指向仓库内根，scope 为 User 而非 Repo。

## Final validation
低磁盘配置 `cargo test --locked --offline -p agent --lib prompt::skills::tests --quiet`：93 passed。新 Unix 临时目录回归覆盖外部链接入仓库、仓库链接出外部和 git root 自身为别名，已有配置、发现、去重、禁用等回归通过。

## Scope
只对 scope 比较值 canonicalize，原始 expanded 继续作为发现输入。未新增相对配置路径的独立测试，也未声称所有自动发现 scope 或递归链接逐文件分类已修复。未执行 UI 同名选择端到端验证。CLI 尚未重新链接。磁盘可用约 69 GiB，target 9.8 GiB。
