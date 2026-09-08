# Verification

- 旧实现两个回归失败：append 保留 write_file 旧缓存而忽略 read_file 配置；None 配置批量重编译未清缓存。
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p hooks --quiet`：226 单元、13 集成、1 doctest 全通过，0 失败/忽略。
- 三入口矩阵覆盖 append/dedup/serde × 合法read_file/非法regex/None，输入故意带 write_file 旧缓存，接纳后仅遵从配置事实。
- 全仓 matcher 赋值搜索确认未发现其他生产 Hook 逻辑依赖仅修改 compiled matcher 来表达配置；parser 正常构造按 configured 模式编译，workspace wire adapter 现在仅负责转换。
- workspace 原往返测试只比较序列化值（无法覆盖 skipped matcher），已增加返回 matcher 实际匹配 Bash 且拒绝 Read 的断言。
- 单独 HookSpec 保持可构造传输类型，保证范围是 registry 接纳边界，不声称外部任意 HookSpec 对象始终缓存一致。

最终 workspace：`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p workspace --lib hook_ --quiet`，13 通过、0 失败/忽略。
