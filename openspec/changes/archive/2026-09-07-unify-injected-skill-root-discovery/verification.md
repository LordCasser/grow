## Before
新增一致性回归旧实现失败：仅返回 child，没有 root。该结果证明此前仅扫描子目录，不表示旧规范已承诺根技能支持。

## After
低磁盘配置 `cargo test --locked --offline -p agent --lib prompt::skills::tests --quiet`：96 passed。测试两种 scope 的根/子技能、重复目录去重；现有 local/server/bundled 优先级测试也通过。仅替换共享发现入口，walk_for_skill_md 的测试引用移到测试 imports。

## Limits
未发现仓内 launcher 注入者，本轮不实现新 launcher，不变更 reset 所有权。CLI 未重新链接。磁盘可用约 69 GiB，target 9.8 GiB。
