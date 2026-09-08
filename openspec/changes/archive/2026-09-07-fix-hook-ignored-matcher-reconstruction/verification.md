## 证据
新增 ignored_matcher_policy_survives_registry_boundaries 在旧实现失败：UserPromptSubmit read_file append 意外构造 matcher。覆盖全部 Ignored 事件、合法/非法模式、parser + append/dedup/serde/refresh 四入口。

第一次全套 228 passed / 1 failed 揭示旧 invalid_restored_matcher_skips_event_without_match_value 测试将 Stop 当成 Tested，违反 EventTraits 和 parser 原行为。未删除该缺失字段场景：改成双场景，SessionStart 空 source 保持 MatcherMiss；Stop 忽略非法模式而 Execute。同步限定主规范的 Never 要求适用于 Tested。

最终 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p hooks --quiet：229 单元 + 13 集成 + 1 doctest 全通过。rustfmt、git diff --check 通过。

## 范围
修正此前重建遗漏事件策略的回归，保持通用 Never 判定及 Tested fail-closed 规则；没有执行用户命令或修改用户配置。
