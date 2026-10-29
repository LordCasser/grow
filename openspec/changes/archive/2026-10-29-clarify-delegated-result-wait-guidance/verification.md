# 验证记录

## 环境与命令

验证使用仓库指定的 Cargo / Rust 1.93.1，PATH 优先 rustup 代理；测试 target 位于工作区外，禁用 incremental，完成后用 Cargo clean 清理。本记录不依赖临时日志或编译产物。

| 验证 | 结果 |
| --- | --- |
| `cargo test --locked -p tool-types --lib` | 125 passed |
| `cargo test --locked -p tools --lib implementations::grow_build::task -- --test-threads=4` | 198 passed |
| `cargo test --locked -p agent --lib prompt::` | 208 passed |
| `cargo check --locked -p shell --lib` | passed，含恢复调用方的生产编译 |
| `RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib wait_interrupt_tests -- --test-threads=4` | 7 passed；覆盖插话中断、原任务继续、纯计时等待、嵌套 guard 及取消后的 guard 清理 |
| `cargo fmt --all -- --check`、`git diff --check` | passed |
| `openspec validate --all --strict --no-interactive` | 归档前 17 passed；归档后 16 passed |
| `openspec validate --archived --no-interactive` | 归档后 743 passed |

## 场景与证据

| 契约/反例 | 核对结果 |
| --- | --- |
| 有独立工作、无独立工作、只需要部分 child 结果 | primary 与 spawn 说明保留工作优先，明确原 ID 的正超时等待；既有输出工具说明和测试保持多 ID wait-all。属于文案/渲染核对，不代表真实模型遵循率。 |
| 实际工具名与参数重命名 | 增强后的 `background_spawn_succeeds_when_coordinator_accepts` 通过真实 `TaskTool::run` 生成回执，断言 `await_result`、`jobs`、`max_wait`、原 ID 和当前 cap；该夹具验证 renderer→回执组装，不宣称覆盖 registry admission。 |
| 生成示例可作为等待输入 | `background_receipt_wait_example_is_callable_with_parameter_mapping` 解析实际回执 JSON，反映射后反序列化为 `TaskOutputToolInput`；覆盖 canonical、完整/部分映射、引号转义、50ms 和 1ms 正额度，并验证 `waits()` 与原 ID。 |
| 零等待额度 | `background_receipt_zero_wait_allowance_has_no_blocking_example` 确认不生成假阻塞示例且保留任务和结果入口。 |
| 超时、running、失败与不存在 ID | 工具测试覆盖正超时等待、cap、零/省略非阻塞、完成/失败/不存在与 observation-only；指引明确等待返回不等于完成/验收。等待 runtime 未修改。 |
| 已完成与注册竞争、多依赖 | coordinator/output 既有测试与源码核对；仍由原 owner 查询/注册，多个 ID 的 join-all 与 deadline 路径不改，不新增 waiter 账本。 |
| 前台转后台 | 既有 `auto_backgrounded_result_returns_task_id_text` 通过真实 task 路径，保留 moved-to-background 状态与原 ID，指引使用正超时等待；不假定 child 已完成。 |
| 树恢复 | 恢复调用方从当前 bridge 的 renderer 读取映射，向共享 formatter 传原 task ID 和当前 cap；生产编译通过。未新增真实树恢复端到端夹具。 |
| View 和输入语义 | 核对既有 `Waiting(TaskOutput)` 与 parked 路径；未改 View 或新增 Task 行。Shell 中断验证结果见上表；未执行 PTY 交互评测。 |

## 对抗性复核与限制

按 audit.md 核对 13 组边界，独立只读复核未发现可复现缺陷。命令专用工具集没有增加子任务说明；没有 sleep 内容拦截、额外任务、持久状态或配置。本次不实现无限等待。

没有运行真实模型行为评测，因此不能承诺所有模型都会停止创建 sleep 占位任务；交付范围是更明确的等待指引和可调用回执。没有构建/替换正在运行的 Grow CLI，改动需要重新构建后生效。

后续已用当前代码完成一次真实模型场景验证，结果通过；模型、完整工具调用顺序和单次场景限制见 [真实模型验证](../2026-10-29-verify-delegated-result-wait-live-model/verification.md)。

## 归档

场景与验证记录核对完成，全部 tasks 已勾选。`openspec archive clarify-delegated-result-wait-guidance --yes` 已将 1 项新增要求和 1 项修改要求合入 local-coordination；归档后全量严格与 archive 校验均通过。
