# 验证记录

## 证据

截图对应主会话 `01a0a318-1c4d-7780-8691-d027ad1fb267`。本地 unified log 记录 2026-09-15 12:28:27、12:28:41、12:29:27、12:29:34 四次模型选择进入 Shell 路由入口。该日志文字 `model changed` 只证明入口被调用，不作为已提交模型的证据。主会话在等待子 Agent 输出，parked 仍是运行中的前台，保持 Step 边界切换。

`render_turn_status` 已收到 `control_status`，但 `state.is_idle()` 专属分支未包含 parked，后续后台任务提示直接返回。新增渲染测试在旧实现失败，实际 buffer 为 `○ waiting · Enter `，完全没有模型转换信息。

## 验证结果

| 验证入口 | 结果 |
| --- | --- |
| `cargo test --locked -p pager --lib views::turn_status::tests:: -- --test-threads=4` | 43 passed，0 failed；新测试覆盖 Subagent/TaskOutput 等待、0/1 子 Agent、Pending/Applying 文案、18/80 列和清除后的提示/点击区 |
| Pager `app::root::dispatch::tests::router::` | 92 passed，0 failed；含模型派发、重连延迟派发及新意图 |
| Pager `app::acp_handler::tests::session_events::` | 43 passed，0 failed；含控制 Pending 覆盖、终态清除和重复/过期结果 |
| Pager `app::acp_handler::tests::models::` | 18 passed，0 failed；含模型广播与本地 pending 归属 |
| Shell `session::actor::model_switch::tests::` | 25 passed，0 failed；含 busy 请求覆盖中间目标、活动 Step 后应用 model/effort、边界冻结 |
| 修改生产文件 `rustfmt --edition 2024 --check`、`git diff --check` | 通过 |
| `openspec validate --all --strict --no-interactive` | 18 passed，0 failed（本 change 归档前） |

测试使用 `RUST_MIN_STACK=16777216`，构建使用 `CARGO_BUILD_JOBS=4`。后三组 Pager 及 Shell 使用 Cargo 已构建的测试二进制按模块筛选运行，避免再次编译。仓库其他测试文件存在既有 rustfmt 差异，未混入本次修改。

## 限制

修复显示遗漏，模型仍待当前 Step 结束后应用。测试证明当前实现的派发、投影及渲染路径，不声称从日志恢复了当时未持久化的瞬时 Pending 通知。未中断用户正在运行的会话或子 Agent，未替换已运行的 Grow 进程。

## 归档与清理

已完成场景核对并归档；归档后的 `openspec validate --all --strict --no-interactive` 为 17 passed、0 failed。全部 Rust 验证结束后执行 `cargo clean`，移除 81,899 个文件、23.3 GiB 构建产物，未清理其他项目。

归档完整性校验 `openspec validate --archived --no-interactive`：336 passed，0 failed；清理后磁盘可用空间约 42 GiB。
