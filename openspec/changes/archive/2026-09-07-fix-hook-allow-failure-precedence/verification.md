# Verification

- 旧实现定向 allow_ 测试：4 通过、2 失败。新增 HTTP 非成功 allow 矩阵和真实命令 on_failure=block 回归均失败。
- 修复后 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p hooks --quiet`：217 单元、13 集成、1 doctest 全通过，0 失败/忽略。
- HTTP 302/403/500 的 allow 均 Failed，deny 保持显式拒绝。真实 dispatcher 运行打印 allow 后 exit 1 的命令，记录 Failed 并执行 on_failure=block 拒绝。
- 既有 allow 成功、exit 2 优先和其他决策回归通过。Stop/Observe 与成功状态下非 JSON 输出规则未改，结构化解析错误已另记 backlog。
