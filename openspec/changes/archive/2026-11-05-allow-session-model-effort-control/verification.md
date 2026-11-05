# Verification

状态：两项功能实现与里程碑集成已验收。定向库测试、默认CLI检查/构建、三个真实CLI用例和格式全部通过；独立静态review无实现阻塞，指出的组合验证缺口已关闭。变更已归档并合入主规范，最终全量strict与归档校验全部通过。以下记录实际结果。

## Environment

仓库固定Rust/Cargo 1.93.1。默认PATH曾实测为Homebrew 1.99.1；验收使用 `PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:$PATH"`，版本输出均为1.93.1。库测试按开发指南设置 `RUST_MIN_STACK=16777216`。系统diff为 `/usr/bin/diff`。CLI集成使用同一工作树 `cargo build --locked -p cli --bin grow` 产物，`GROW_BINARY="$PWD/target/debug/grow"`；mock provider均为隔离loopback服务，不调用外部模型。

## Acceptance matrix

| 契约/场景 | 实际入口和断言 | 已取得结果 |
| --- | --- | --- |
| Resume四种字段组合、null、none | Task JSON→runtime，真实child runner/provider | tools Task 64/64、最终provider matrix 1/1 |
| 最后durable配置 | 来源actor切换后完成，provider/Spawn/baseline一致且来源不变；包括None effort | 最终provider matrix 1/1；真实新工具组合CLI通过 |
| 非法/unsupported/unknown/nonselectable | 明确错误、零child provider、无替代或fresh fallback | matrix通过；模型catalog错误路径5/5 |
| 来源canonical/workspace/window/权限 | canonical/security/cancelled/live/context/ceiling及source immutable既有回归 | subagent 243/243，包含新checkpoint/模型选择约束 |
| 原身份checkpoint restore | 挂起、冷restore仍保留birth authority/frontier，而非使用Task override | end-to-end 1/1 |
| 工具参数/目标绑定 | 严格字段、至少一项、null、malformed effort；host-bound当前session/incarnation | tools新工具2/2；真实Shell矩阵通过 |
| 工具→下一provider | 不自锁，原完整call/result一次，下一request使用新route/effort；跨Chat/Responses，rebuild绑定 | Shell session_model筛选11/11，其中主链测试隔离运行16种场景 |
| effort-only与用户pending model | actor按最新desired model合成，不恢复旧model | 16场景中的pending-user-model通过 |
| 拒绝/关闭/失败 | denied/Hook/child ceiling/精确permit，以及unknown/hidden/blocked/unsupported/关闭receipt | Shell/工具通过；Hook有Triggered/Blocked/Completed事实，不只断言route不变 |
| Workflow | frozen Run route与host绑定，不替换全局route | 实际binder Workflow测试通过 |
| 默认产品构建 | 当前源码默认feature CLI check/build | 两命令通过，exit0 |
| 独立review | 实际diff、共享控制/恢复调用方 | luna静态review接受；跨能力CLI证据缺口已补case并通过真实运行 |
| 文档/OpenSpec | 用户/开发者说明与场景 | 说明已更新；归档前18/18、归档后全量strict16/16与archived848/848通过 |

## 实际命令与结果

所有Cargo命令均使用上述固定PATH/RUST_MIN_STACK，串行协调，无并行重型Cargo。测试产物打印的selected count已核对；重复运行不叠加为更多独立覆盖。

| 命令 | 实际结果 |
| --- | --- |
| `cargo test --locked -p tools --lib implementations::grow_build::task::tests -- --test-threads=1` | 64 passed，0 failed，0 ignored |
| `cargo test --locked -p shell --lib agent::mvp_agent::tests::task_resume_presentation_tests -- --test-threads=1` | 2/2，179.50s；真实runner、canonical source/transcript。之后对最终endpoint断言单独复验如下 |
| `cargo test --locked -p shell --lib agent::subagent:: -- --test-threads=1` | 243/243，12.73s |
| `cargo test --locked -p shell --lib agent::mvp_agent::tests::subagent_restore_authority_tests -- --test-threads=1` | 1/1，210.19s；`suspended_child_cold_restore_retains_birth_authority_end_to_end` |
| `cargo test --locked -p shell --test test_subagent_effort --no-run` | 测试target编译通过；不算ignored运行证据 |
| `cargo test --locked -p tools --lib set_session_model -- --test-threads=1` | 2/2，含closed admission receipt不能返回queued |
| `cargo test --locked -p shell --lib session_model -- --test-threads=1` | 最终11/11，23.03s；主链内部16个独立进程场景，Workflow和exact permit各真实选择执行 |
| `cargo test --locked -p shell --lib task_resume_selection_reaches_provider_and_preserves_canonical_source -- --test-threads=1` | 最终1/1，98.65s；断言真实/v1与/v2 endpoint、来源最后route/effort、组合/失败、derived事实 |
| `cargo test --locked -p shell --lib fresh_tool_model -- --test-threads=1` | 最终5/5，0.00s；共享错误文案改动也覆盖 |
| `cargo check --locked -p cli` | 默认feature通过，1m23s，无最终warning |
| `cargo build --locked -p cli --bin grow` | 默认feature通过，1m37s，无最终warning |
| `cargo fmt --all -- --check`、`git diff --check` | 最终无输出通过 |

最终Shell三命令串行链exit0，总168.30s；CLI check/build串行链exit0，总181.38s。

## 真实CLI组合验收

`GROW_BINARY="$PWD/target/debug/grow" cargo test --locked -p shell --test test_subagent_effort -- --ignored --test-threads=1`：最终3 passed，0 failed，0 ignored，0 filtered out；执行28.96s，含编译总135.22s，exit0。三个用例全部实际运行：

1. `subagent_tool_effort_reaches_provider_and_rejects_invalid_selections`：fresh high/none到真实child provider，unsupported/bogus零错误替代。
2. `subagent_resume_tool_patches_model_and_effort_at_the_provider`：真实父spawn来源并完成，省略/null/model-only/effort-only/both及各错误路径，provider与来源字节对照。
3. `subagent_resume_inherits_a_tool_selected_route_with_unset_effort`：三个endpoint隔离真实父/source/最终模型；source调用set_session_model切到无efforts模型，完整queued交换后以None effort完成；父省略字段resume沿最终route/transport/None；derived Spawn/唯一baseline一致、source出生事实和Timeline字节及parent选择不变。

## Review与中途失败处理

实现由sol coder按两个独立行为交付，luna独立静态review检查实际最终diff，父核对关键owner/共享状态与实跑结果。review发现普通resume不能只读出生Spawn，已使用完整校验后child最后durable selection；主命令选择类型归session commands，避免session依赖agent handler；工具不等待自己Step的applied ACK。其余审计债务移至独立全仓审计，不夹带修复。

中途失败均保留事实：Task测试后的早期Shell编译发现新增ToolInput穷尽匹配缺项，已在既有InternalControl投影补齐；中途SessionSamplingSelection可见性warning已消除；首次真实新工具11项中Hook fixture使用外部PreToolUse格式未被native parser采纳，10/11后管道停止，后续命令当时未执行。fixture按原生pre_tool_use与GateHookJson修正并增加Hook执行/阻断durable事实，最终11/11。没有为变绿删除场景、放宽parser或宣称失败管道全部通过。

## 场景核对与限制

三个delta已逐项对照实现和上述测试：字段独立覆盖、source最后durable值、canonical cancelled/live/security/workspace/context及checkpoint分流；新工具queued/下一请求/最新desired/拒绝无mutation/后续application语义；host绑定/伪造目标拒绝/Hook和permit/Workflow与能力不扩张。application晚失败/被supersede复用既有Sampling control/finish owner，静态核对无新平行状态；本次没有制造真实磁盘故障或外部provider native拒绝。

未运行全workspace全部测试、跨平台kernel/TTY、真实付费provider或网络故障campaign。无关的全仓各版本审计结论和独立债务见另一change，不以本矩阵代替全仓验证。未创建Git提交、合并或发布。

## OpenSpec

两change定向strict及归档前全量strict已通过（18/18）。`openspec archive allow-session-model-effort-control --yes`成功更新三份主规范（+2、~3 requirements）；独立审计以 `--skip-specs --yes`归档。两目录均为2026-11-05前缀。

归档动作对应的末项当时保留未勾选，CLI提示8/9和11/12；首次archived检查仅因此两项失败，归档和搬迁链接核对完成后勾选再校验。最终 `openspec validate --all --strict --no-interactive` 16 passed、0 failed；`openspec validate --archived --no-interactive` 848 passed、0 failed，串行链exit0。25份本轮相关文档本地目标无断点，所有tasks完成。
