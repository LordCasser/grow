## Evidence and final behavior

用户截图中的 `Plan artifact: artifact:plan:unavailable` 来自 `actor/tool/result.rs` 对所有 PlanControl 结果的无条件文本覆写。Complete/Cancel 在结果投影前已持久退出 Normal，当前 Plan runtime 的 hash 已清除；原始完成报告也因此被遮蔽。

最终实现遵循用户追加要求：完整原始 report 写入既有不可变 artifact 存储，完成计划身份与报告引用作为 Control 快照的一部分保留；顶部显示 Complete，详情以 Plan / Result report Tab 查看两份内容。Complete 成功后在 Normal 下重新采样，模型按工具和执行阶段说明给出简短正常回复。工具结果只确认报告已保存。下一次成功手动选择 Behavior（含当前 Normal）清除完成展示，自动控制和恢复不清除；fork 不继承完成展示身份。

## Validation scope

复用本 chat 已附加的隔离验证副本，代码基线为 `cd5d88ec` 加已提交的 `f99310f7` Plan/TODO 源码补丁，再叠加本次改动。独占文件逐字节复制；`spawn.rs`、`session_admin.rs` 和 Pager `session_notification.rs` 只叠加本次片段。原工作区的 MCP / ImageRecovery 改动未带入构建。

独立校验共享文件：从当前内容反向剔除本次片段后，SHA-256 与前一提交前的工作区快照完全一致；其他既有未提交文件的字节亦保持不变。

## Scenario reconciliation

| 场景 | 验证 |
| --- | --- |
| 完成后完整报告与简短确认分离 | `plan_control_results_preserve_lifecycle_messages` 实际工具派发，检查 ACP content/raw、Timeline 工具结果仅含确认，报告 blob 与输入原文逐字节一致，Control 完成引用可恢复 |
| 取消、非法完成、提交与修订 | 同一矩阵覆盖 Executing/Drafting Cancel、缺 report 的失败、Submit/Amend 审批成功及对应状态 |
| 手动同/异 Behavior 清理 | `completed_plan_clears_only_after_successful_manual_behavior_selection` 检查共享控制 gate、拒绝保留、Normal/Clarify 成功后内存与持久状态均清除 |
| 报告写入及控制关闭失败 | `plan_completion_storage_and_control_failures_preserve_execution` 检查超限写入失败和关闭准入时状态不变、无完成投影 |
| 自动 Goal 保留与恢复 | `automatic_goal_activation_preserves_completed_plan_control_state`、`goal_definition_reactivation_preserves_completed_plan_control_state` 核对内存、Control 与恢复一致 |
| fork 不继承完成展示 | `forked_completed_plan_keeps_history_without_inheriting_the_status_entry` 核对源状态保留、fork 状态清除 |
| 详情双 Tab 与只读交互 | Pager `completed_plan_viewer_switches_read_only_tabs_by_keyboard_and_mouse` 使用实际渲染命中区及键鼠路由；关闭反馈/审批，保留选择与复制 |
| 详情打开时收到完成投影 | `completion_projection_refreshes_an_open_plan_detail_once`、`completion_projection_upgrades_an_already_open_plan_preview` 核对单次 canonical 查询和已完成报告展示 |
| 状态栏、手动清理及迟到查询 | `completed_plan_stays_inspectable_until_a_behavior_change_applies`、`completed_plan_clear_retires_a_pending_preview_response`；同模式清理及旧响应均覆盖 |
| session 重新绑定 | `completed_plan_does_not_cross_a_session_binding` 检查同 ID 保留、不同 ID 清标记和旧查询身份 |

## Checks

- `git diff --check`：通过。
- OpenSpec 实现阶段全量严格校验：17 项通过；归档后全量严格校验 16 项通过，归档校验 614 项通过。
- `CARGO_TARGET_DIR=<主工作区>/target CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p shell -p pager -p tools --lib plan_ -- --test-threads=4 --quiet`：Pager 141、Shell 89、Tools 14 项全部通过。
- 同一最终二进制的 `agent_switch_`：6 项通过；`usage_status_tests`：4 项通过。
- 隔离副本中全部本次 Rust 文件的 `rustfmt --edition 2024 --config skip_children=true --check`：通过。

首次测试编译修正了测试断言的字段访问，以及新增 UI 代码中的键位宏、方法可见性和静态标签宽度调用。随后修正了两处 fixture 预期：未知 Behavior 返回 `Rejected` 而非异常，Clarify 的正式 wire ID 为 `ask`；还将报告先于权威完成投影到达的测试调整为拒绝、投影到达后再读取。未为通过测试更改这些既有契约。

## Adversarial review and limits

完成候选在共享 step/Goal gate 下准备，Control 确认后才替换 live Behavior，canonical 读取不会见到未提交的完成引用。报告写入失败或控制提交失败保持原状态；取消不会生成完成报告。canonical 查询结束时同时核对计划与完成报告引用，避免读到混合版本。

仅成功的手动 Behavior 接纳清除结果，包括相同模式的持久确认；确认中和拒绝保持完成标记。自动 Goal 的创建/激活/定义变更也显式保留完成引用，防止内存与 Control 分裂。前端只按权威完成标记显示状态，清理时递增查询身份；完成详情不具备评论或审批操作。TODO 的独立入口保持。

验证使用离线 actor、真实 Ratatui Buffer 与输入路由，未运行真实 provider、交互式 PTY 或安装新二进制。正常简短回复以 Complete 的 ResampleStep、Normal 控制上下文和提示约束验证，未评估真实模型生成文字的质量。历史已经发送的占位文字不迁移。

## Cleanup

`cargo clean -p shell -p pager -p tools` 删除 4,683 个文件，共 26.2 GiB，保留可复用的其他依赖缓存。验证副本沿用已附加工作树；其既有 Codex 保护状态不绕过。
