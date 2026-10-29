# 验证与范围

日期：2026-10-29，Rust 1.93.1，macOS arm64。根分析与两项独立只读审查分别核对请求投影/状态变化、usage/协议归一化/客户端展示。完整结论见 [audit.md](audit.md)。Atlas 局部查询未覆盖的调用关系由直接源码核对，不宣称全仓调用图完整。

## 本轮执行

- 修复前实际运行新回归 `goal_scope`：0 passed / 2 failed。路由替换后 36 vs 46026，图片 route 失效后 2036 vs 48026；[证据](goal-pressure-before.txt)。首次 Cargo 使用 active_goal 过滤为 0 项，未作为验证，改用实际测试名对应过滤执行。
- 修复将字段从 ImageBudgetSelection 迁移到 ChatState.request_active_goal；不保留第二份 tag。successful build 替换此作用域，路由/图片缓存清除不再使其丢失；路由替换仍重置校准，普通图片失效保留既有校准。
- `cargo test --locked -p chat-state --lib -- --test-threads=4`：**597 passed / 0 failed / 3 ignored**，20.69s；[输出](chat-state-results.txt)。含 3 项新增测试：两种失效路径的即时计数、事件语义和重复 build，以及 active → None → new active。
- 共享原语 `cargo test --locked -p token-estimation --lib`：**15 passed / 0 failed**；[输出](primitives.txt)。
- 非测试生产配置 `cargo check --locked -p chat-state`：通过，11.81s；[输出](production-check.txt)。本轮未重跑完整 CLI/Pager 集成，不引用前轮构建作为本轮执行结果。
- 相关三个 Rust 文件 rustfmt 检查、git diff --check：通过。测试初版将计数未变化的图片失效也期望成事件发布；已按现有“变化才发”的语义校正，保留数值和 rebuild 断言。

## 审计限制

usage/protocol/UI 结论主要来自直接源码和既有测试清单，未在本轮调用线上 provider，也未重新执行全部 Shell/Pager 测试；相关证据强度在 audit.md 单列。低于 heuristic 的 provider anchor 策略、普通请求输出预留及恢复 UI 组合列为 backlog。冷恢复首个请求前的 Goal scope 初始化仍需独立集成核对，不能从历史最后一个 tag 猜授权。

修改保留在原工作区，未提交或发布。未改无关已有脏文件。

归档后全量严格校验 16 项通过、archive 校验 721 项通过；cargo clean 回收 614 MiB。
