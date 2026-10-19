## Session evidence

只读检查 `~/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fskynet/01a15323-b21a-7403-906e-72048e8b7594/`，未启动或写入该 session。以下时间使用 Asia/Shanghai，未复制用户任务正文。

- `timeline.jsonl:3,8`：2026-10-19 15:50:10 / 15:50:18，Agent 从 grow 切换为 software-engineering/software-architect。
- `timeline.jsonl:509,514`：Plan 从 AwaitingApproval 进入 Executing。
- `timeline.jsonl:529,535,536`：16:19:16 左右实际调用了 `todo_write`，工具返回 `Planning runtime is unavailable`，outcome 为 error。不是工具 schema 缺失或权限拒绝。
- `updates.jsonl:1`：唯一 ACP plan 更新来自初始化，`entries=[]`；后续只有 Plan mode 投影，没有成功写入后的 TODO 列表。

## Goal comparison and cause

Goal 的 `goal_notification.rs::GoalNotifySender` 从持久 Goal 快照发布 GoalUpdated，Pager `apply_goal_update` 保存 `GoalDisplayState`，`agent_status.rs::goal_status_line` 在顶部生成状态/用量/耗时，`hit_goal_status` 打开 `goal_detail`。状态入口不依赖 TODO。

Plan 已有 mode/phase 投影和 canonical artifact 查询，但 `should_show_plan_chip` 需要正文缓存，审批视图清理后可能失去入口。TODO 的 PlanningRuntimeHandle 则仅在启动绑定，Agent 激活的资源替换会丢失该瞬态依赖。修复沿这两个既有 owner 分别补齐，不引入新状态权威。

## Validation

- `git diff --check`：通过。
- `openspec validate --all --strict --no-interactive`：实现阶段 17 项通过；`openspec archive fix-plan-progress-runtime --yes` 归档后 16 项通过。`openspec validate --archived --no-interactive`：613 项通过。
- 主工作区 Shell 测试二进制 `shell-b3ef0e71c856832d agent_switch_ --test-threads=4`：6 项通过，包括新回归、Agent 角色历史保留、busy 切换与子 Agent 能力边界。
- 新回归首次运行的末尾 ACP 断言读了 fixture 未转发的 gateway 通道，结果为 0；修正为 SessionActor 实际事件出口后上述 6 项全部通过。首次测试的 TODO 创建、更新、状态和拒绝断言已通过。
- 主工作区 `cargo test --locked -p pager --lib plan_` 与随后已排队的组合构建均被既有 `session_notification.rs` / `scrollback/block.rs` 的 5 个 ImageRecovery 缺失定义错误阻止。未删除、回滚或补写这些无关改动。
- 隔离验证基线为 `cd5d88ec7f5d5c42ee18277304c352232dcb4e14`，仅叠加本次六个 Rust 文件的变更。五个独占修改文件与主工作区逐字节一致；`spawn.rs` 只应用本次 Planning handle 删除块，保留主工作区其他改动。
- 隔离 `CARGO_TARGET_DIR=<主工作区>/target CARGO_BUILD_JOBS=2 cargo test --locked -p shell -p pager --lib plan_ -- --test-threads=4`：构建成功，Pager 132 项通过；Shell 在既有 `defer_ack_retry_after_control_rebuild_keeps_one_terminal_and_never_resamples` 遇到默认测试线程栈溢出。用 `RUST_MIN_STACK=16777216` 运行同一已编译 Shell 二进制的 `plan_` 分组后 82 项全部通过；`agent_switch_` 分组 6 项全部通过。
- 隔离 Pager 二进制 `usage_status_tests --test-threads=4`：4 项通过，包含 Goal 优先级、普通用量、子视图与 Plan/TODO 并存命中区。

## Scenario coverage

| 场景 | 验证 |
| --- | --- |
| 首次 TODO 前切换 Agent | `agent_switch_preserves_planning_runtime_and_committed_todos` 第一次替换后真实 ToolBridge 调用成功 |
| 带已有计划切换 Agent | 同一回归第二次替换核对 scope、Timeline seq、两个投影及合并后的旧步骤；每次写入一个 ACP plan，替换本身为零 |
| 无缓存/无 TODO 的 Executing | `plan_status_without_cached_body_opens_details_and_keeps_todos_separate` 实际 Buffer 文本和命中区；`plan_details_load_full_artifact_without_cached_body_and_preserve_todos` 查询并打开完整正文 |
| Drafting 无 artifact | 上述渲染回归覆盖 Drafting，详情回归覆盖无 artifact 的错误返回，不打开旧缓存 |
| 阶段更新与退出 | 四个阶段逐项渲染并点击；退出后 Plan 入口消失、TODO 保留 |
| Plan 详情与 TODO 并存 | 独立 hit area、TODO 面板打开、查看/关闭详情均保留 in_progress 状态；Goal 原状态栏回归同时通过 |

## Adversarial review

- Agent 激活处在原有 foreground/step gate 内，候选先读取权威计划并绑定原 actor sender，再提交激活；读取失败沿现有错误路径返回。未扩展工具 allowlist、RWX 或审批权限。
- `todo_write` 仍检查当前 prompt 的 foreground ownership。新回归确认 turn 结束后仍拒绝调用；更换 Agent 不产生新的 scope/revision，合并更新不丢失旧步骤。
- Plan 入口仅改变展示条件和标签。点击复用 session/binding epoch/request identity/hash 验证；失败不打开旧正文，查看不提交审批或执行。
- TODO 独立入口与状态不依赖 Plan 详情的打开/关闭；Plan 退出不清空 TODO。
- 另发现同步 compaction 的前端 TODO 清理分支，已单独登记 backlog 待核查，未扩展本次实现。

## Limits and cleanup

未重放真实 provider、未操作用户运行中的 session、未安装或发布新二进制。TUI 验证使用实际 Ratatui Buffer 和输入路由，未运行交互式 PTY smoke。

`cargo clean -p shell -p pager` 清理 2,201 个文件，共 21.9 GiB，保留可复用依赖缓存。隔离 worktree 的 Codex 归档请求返回 `This worktree is protected by a pinned task or workspace`，因此保留该受保护副本，未绕过保护删除；构建输出位于已清理的主工作区 target，不在副本内。
