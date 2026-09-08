# Verification

- 旧实现定向回归失败：Never + None 返回允许。
- 修复后 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p hooks --quiet`：224 单元、13 集成、1 doctest 全通过，0 失败/忽略。
- 匹配器回归覆盖 None、空值、工具名均拒绝；有效匹配器遇到 None 和完全未配置仍允许。
- dispatcher 回归把无效模式通过 recompile_matchers 恢复为 Never，再为无匹配字段 Stop envelope 生成计划，验证 MatcherMiss 而非 Execute。
- 未启动真实 Hook 子进程；本改动发生于执行前计划，计划回归直接覆盖执行准入边界。
