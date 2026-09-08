# Verification

- 旧实现定向两个 runner 回归均失败，{"decision":false} 被当作日志允许。
- 首轮修复全测试暴露空数组被 Serde 解析为默认 Stop，以及旧截断 JSON 允许断言；新增对象协议入口拒绝数组，明确更新旧断言而保留普通文本允许场景。
- 最终 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p hooks --quiet`：220 单元、13 集成、1 doctest 全通过，0 失败/忽略。
- 矩阵覆盖命令/HTTP、Prompt/Tool 与 Stop 解析，字段类型错误、未知字段、截断对象、空数组均 Failed；Prompt/Tool exit 2 在协议错误及未知决策时仍 Deny。
- 真实 dispatcher 执行输出错误 JSON 的命令：Allow/Block 两种失败策略均记录 Failed，只有 Block 最终拒绝。
- 本次不是任意文本 JSON 提取器；普通文本仍容忍，不尝试从日志中提取嵌入 JSON。Stop 已有有效 JSON 与退出码优先级不变。
