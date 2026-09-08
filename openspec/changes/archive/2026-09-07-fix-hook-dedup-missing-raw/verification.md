# Verification

- 旧实现定向回归失败：缺失 raw 的不同命令仅保留 first，second 消失。
- 最终 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p hooks --quiet`：221 单元、13 集成、1 doctest 全通过，0 失败/忽略。
- 回归覆盖命令与 HTTP 两种规格：raw None 时不同实际值保留，相同实际值重复项被去除，顺序仍为 first、second。
- 既有跨来源同内容去重、高优先 timeout/env 胜出、matcher 分离等回归通过。未改变 raw 存在时的主键，也未执行端到端 Shell UI 加载测试。
