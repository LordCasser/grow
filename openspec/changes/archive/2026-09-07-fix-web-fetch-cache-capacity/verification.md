# Verification

- 旧代码运行缓存测试组：1 通过、2 失败，分别复现零容量仍可命中，以及满容量更新 b 错删 a。
- 修复后 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p tools --lib implementations::grow_build::web_fetch:: --quiet`：130 通过，0 失败，0 忽略。
- 回归覆盖容量 0、多次插入、容量 2、重复 URL 内容/时间更新、其他页面保留、新 URL 最老淘汰和条目上限。手工控制缓存时间戳，不执行等待或访问网络。
- 完整组包含上一轮模型切换预算回归，保留缓存命中输出预算处理。
- 未验证真实远程请求；本次变更不涉及传输路径。
