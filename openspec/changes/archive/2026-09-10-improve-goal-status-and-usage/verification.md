# 验证记录

日期：2026-09-10。

## 源码范围

真实会话只读调查见 [session-analysis.md](session-analysis.md)。26 次查询均成功，报告耗时为 70–340 ms；26 次都缺失 ACP Completed。源码中遗漏的转换分支与 Pager 等待 turn 收尾的机制吻合。

共享工作区同时实施 `unify-sampling-attempt-recovery`。首次在共享目录执行 chat-state 新增用量事件测试通过；后续 shell/pager 编译在当时的 sampler 修改处遇到 E0502、错误类型转换及非穷尽匹配，另一次在 chat-state 状态重建处遇到 E0282。因此采用提交 `c03a62fae8f879e93321d8c00cd4ad1ac6191ec7` 加本 change 代码的隔离源码快照验证，未修改共享索引或移除其他任务代码。

快照的路径、覆盖文件、SHA-256 和排除范围见 [validation-source.json](validation-source.json)。快照初次复制带入了并发任务的新结算测试，编译报不存在的 `settle_model_attempt_usage`；已从固定基线重新构造该测试文件，只保留本 change 新增测试，随后验证通过。开发中也修正了 Goal 历史用量 `i64` 传给 `u64` 格式函数的类型错误。

以下通过结果对应隔离快照，不代表并发施工中的整份工作区已通过集成验证。

## 执行结果

构建参数：`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216`；快照使用共享 `CARGO_TARGET_DIR`，未执行 cargo clean。

- `cargo test --locked --lib -p shell -p pager --no-run -j 4`：通过。
- Shell 测试：`session::acp_conversion::tests`、`session_usage_projection_is_transient_and_matches_usage_query`、`goal_admission_tests`、`session::goal_tracker`，共 **65 passed**。
- 最终 `cargo test --locked --lib -p chat-state -p pager -j 4 -- <filters> --test-threads=4`：新增账本测试 **1 passed**；Pager 用量、详情、点击、重连与已有 Usage 面板测试 **44 passed**。
- 对最终 chat-state 测试二进制执行完整 lib 测试：**467 passed, 1 ignored, 0 failed**；ignored 保持原有测试标记，未为本次改动忽略场景。
- 对最终 Pager 测试二进制执行 `app::agent_view::render:: acp::tracker::tests --test-threads=4`：**163 passed**，覆盖公共状态栏绘制与原工具行生命周期。与上面的 44 项有重复，不相加作为独立测试数。
- 链接器报告 `__eh_frame` 超过 compact unwind 编码大小的警告，构建退出成功。
- `git diff --check`：通过。
- 归档前 `openspec validate --all --strict --no-interactive`：**19 passed, 0 failed**。

Pager 的 44 项筛选条件为：`goal_read_result_finishes_running_row_before_turn_end`、`transient_session_usage_replaces_totals_without_context_or_scrollback`、`views::goal_detail::tests`、`views::agent_status::tests`、`app::agent_view::render::usage_status_tests`、`app::agent_view::session::reconnect_usage_tests`、`app::root::dispatch::tests::status`、`app::status_blocks::tests`。

未执行 release 构建、真实交互终端的人工点击或整个 Rust workspace 全量测试。绘制和鼠标测试调用实际 AgentView / ratatui Buffer / mouse 分发；工具行回归使用实际 `acp_tool_update` 输出，不以手写完成状态代替后端转换。

## 场景核对

| 场景 | 证据与结果 |
| --- | --- |
| Goal 查询/创建在 turn 内完成 | `goal_outputs_emit_completed_updates_with_original_identity` 覆盖 Create/Get/Update 原 id、Completed 和结构化输出；`goal_read_result_finishes_running_row_before_turn_end` 按真实初始调用、标题更新、后端转换结果顺序收尾，无需 finish_turn。通过。 |
| Goal 更新/错误与生命周期 | 完成投影补齐成功变体，错误处理仍走原工具错误路径；转换测试与 Goal admission/tracker 共 65 项通过，预算、完成与阻塞判定未变。 |
| 有上下文时按需读取 | 人工核对行为文件、续跑指令和 GetGoal 说明均已更新；原完成审计全文仍在。提示不能保证模型永不重复查询。 |
| 需要新鲜状态时仍可查询 | GetGoal 执行、工具注册、权限及 Session 查询所有权没有改动；只改工具说明和结果 UI 投影。 |
| 大数、不完整、预算与历史格式 | `detail_groups_large_token_counts`、Goal 详情已有分类测试和 Usage 格式测试通过。完整数字使用逗号，保留 ≥；底栏保留 k/M。 |
| 主调用与晚到子任务 | 新账本事件测试验证累计 120 → 1,100、缓存输入 500/1,000、父 prompt 仍为 120；子任务归属账本的逻辑保持不变。通过。 |
| 空账本/非法缓存比例/不完整 | 底栏测试覆盖未知、无输入、缓存超过输入、大数和 recorded 标记；重复 incomplete 不再重复发事件。通过。 |
| Usage 点击、Goal 优先、窄屏 | 实际 draw 命中区域和 mouse 分发测试通过；已有 ShowUsage 打开 Usage 页测试通过；公共状态栏在窄区域保留右侧项目，点击范围不越界。 |
| 重复/迟到快照和上下文隔离 | ACP 消费测试验证重复替换、拒绝倒退和 replay，不改 context token 或 scrollback；shell 投影与 Usage 查询共用 PromptUsage，transient 通知不写持久化。通过。 |
| 重连/恢复计数窗口 | 客户端 reload 清空旧窗口测试和新账本零值测试通过；源码核对新建/重连沿现有 AdvertiseCommands 补发当前账本。未执行跨进程真实终端重连。 |
| 子视图目标 | 已有 Usage 面板固定定位父会话；本次不在子视图加入错误入口，排除测试通过，独立债务见 backlog。 |

## 归档

`openspec archive improve-goal-status-and-usage --yes` 已合入 4 项新增要求并归档。归档时唯一未勾选项为归档操作自身（3.3），首次 archive 校验因此报告 1 项未完成；操作完成后已勾选该项并重新校验。归档后全量严格校验 **18 passed, 0 failed**；最终 `openspec validate --archived --no-interactive` **303 passed, 0 failed**。全部 8 项任务已完成。
