# 验证记录

日期：2026-10-31。环境：macOS arm64，Rust/Cargo 1.93.1。

## 测试环境与边界

共享工作区存在其他任务的并行修改。本轮使用基线 `e4d82ebd6` 的隔离 detached worktree，应用前两项 Plan 修复与本轮 18 个相关源码/测试文件，逐字节核对与主工作区一致。验证不代表无关并行改动已经通过。使用 `CARGO_INCREMENTAL=0`、`CARGO_BUILD_JOBS=4`、dev/test debug=0、`RUST_MIN_STACK=16777216`；PTY 的 `PAGER_BINARY` 指向该工作树实际构建的 `grow`。

PTY 启动真实终端、Pager、Shell 和持久存储，模型端使用确定性 mock provider。它验证协议与交互交接，不验证线上模型的规划质量或所有终端平台组合。完整分支映射见 [audit.md](audit.md)；状态/故障分支由 Actor 和渲染回归覆盖，没有把所有状态组合宣称为真人手工点击测试。

## 已执行回归

| 层级 | 执行范围 | 结果 |
| --- | --- | --- |
| Shell | `cargo test --locked -p shell --lib plan -- --test-threads=1` | 134 passed，含 amend artifact 写失败与 cancel Control ACK 永久失败 |
| Pager | `cargo test --locked -p pager --lib plan -- --test-threads=1` | 174 passed |
| Minimal | 本轮编译的 `pager_minimal` 测试产物，全部单元测试 | 102 passed |
| Behavior | Shell 编译产物 `session::behavior::tests::` | 34 passed，含执行中 amend、延期 amendment、恢复与连续反馈 |
| 访谈 UI | Pager 编译产物 `views::question_view::tests::` | 55 passed |
| 访谈工具 | `cargo test --locked -p tools --lib ask_user_question -- --test-threads=1` | 61 passed |
| 预览布局 | Pager 编译产物 `views::file_search::line_viewer::tests::` | 13 passed，含窄屏、retained Review 和 pending footer |

各过滤集合可能重叠，不相加作为独立测试总数。非 Cargo 直接运行的测试也先定位本轮编译产物并检查所需测试名称，避免误执行旧产物或零项。

六项 PTY 均通过 `scripts/run_exact_cargo_test.py` 唯一枚举再执行，分别确认 **1 passed / 0 failed / 0 ignored**：

1. `pager-pty-harness --test plan_approval_resume plan_two_revisions_complete`：v1 行内批注与自由文本一起修订，下一次模型请求同时包含二者；v2 第二次自由文本修订到 v3；批准后真实执行 `plan_control complete(report)`，核对持久 Control、报告 hash 和文件原文，以及正常总结请求含成功工具结果。全程不手动核对确认。
2. 同 target `plan_approval_restored_after_resume`：未决定的原始审批经退出/冷恢复重挂，批准后仅一个实现 turn。
3. 同 target `deferred_plan_reviewed_after_resume`：延期自动确认后只打开一次 `/view-plan` 即看到 Review；退出/冷恢复后审阅并批准，不因延期采样实现工作。
4. `pager --test pty_e2e_minimal --ignored minimal::minimal_parked_plan_commits_to_scrollback::minimal_parked_plan_commits_to_scrollback`：Minimal 初版及修订版各进入 scrollback 一次，批准路径正常。
5. 同 target `minimal::minimal_parked_plan_survives_quit::minimal_parked_plan_survives_quit`：待审批正文在退出后保留可见记录。
6. `pager --test pty_e2e_smoke --ignored plan_revise_empty_enter_does_not_approve::plan_revise_empty_enter_does_not_approve`：空 Enter 保持审批并提示输入修订意见，不误批准。

新增连续修订测试已接入 `core-regression.yml` 的既有精确测试门禁。本轮没有提交、发布或更新用户安装的二进制。

## 对抗审查与失败记录

- 窄屏测试使用 32 列，核对各按钮的实际命中区域与内容 viewport 不重叠；鼠标测试从实际 render 结果点击 revise/check/collapse，验证路由及收起后仍保留同一决定。
- retained preview 必须在非 approval 状态保留 `r review`；完成详情保持只读；普通批注发送仍是 `Action::SendPrompt`，只改正聊天/队列提示。
- Minimal 相同提交重建 PAV 时，即使 PAV 自身没有 decision_response，全局 pending 仍必须显示核对确认；空修订输入、收起和 Commenting 的提示分别与真实输入路由匹配。
- 新增测试最初有一处 `u16` 引用比较的编译错误，已修正，并加强为内容边界必须位于第一行按钮之前。首次 PTY 的 mock Chat stream 两个 chunk 使用不同 response id，被真实协议校验拒绝；统一 id 后完整旅程通过，没有放宽协议检查。
- cancel 的永久 Control 写故障会关闭 writer，后续 durable tool-completion ACK 返回错误。测试最初错误地期望 Continue 与仍可 materialize；按生产持久化契约修正为明确错误闭合，保留原 live/外部 durable 快照、artifact、权限和无成功投影断言，没有改生产失败处理。
- 外部 ACP client 的 malformed-response 同连接恢复，以及外部删除不可变 artifact 与恢复并发的观察已独立登记 backlog；正常 bundled Pager 未证实这些触发条件。本轮没有增加自动重试或改通用模态优先级。

## 收尾验证

最终 Shell 134 项回归全部通过，两个新增失败注入均按预期保持原已批准状态，不发布成功决定或 Normal 投影。18 个相关源码/测试文件最终逐字节与主工作区一致。隔离工作树 `cargo fmt --all -- --check` 与主工作区 `git diff --check` 通过；归档后 OpenSpec 全量 strict 校验 16 项通过，archived 校验 757 项通过。验证结束后清理本轮隔离工作树及其约 13 GiB Rust 编译产物。
