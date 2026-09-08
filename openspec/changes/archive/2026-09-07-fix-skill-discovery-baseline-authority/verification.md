## Evidence and results
旧实现 rediscovery_cannot 过滤器执行 2 项，均失败：原始副本错误进入动态可见集合。
修复后完整 Cargo tools lib skill_discovery_tracker 90 项通过。额外真实链路 direct_skill_read_preserves_disabled_baseline 1 项通过：临时 .grow/skills/known/SKILL.md，真实解析形成 baseline 并停用，实际 SkillDiscoveryReminder 读取并再次解析文件，最终仍停用且无重复 pending。

中间 reminders::skill_discovery 在添加上述集成测试前运行 0 项，不计入覆盖。最终增添的是实际测试，无 stub。

构建环境：CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216。命令：cargo test --locked --offline -p tools --lib <filter> --quiet。

范围：只保证已加载同路径配置和门控权威；不声称未知动态路径也经过完整配置过滤，不构成对全部技能权限边界的审计结论。
