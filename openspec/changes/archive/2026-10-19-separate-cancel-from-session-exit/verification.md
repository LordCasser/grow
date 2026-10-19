# Verification

## 最终授权与范围
用户最终明确 Ctrl+C 保持输入框及取消当前回合、不再关联退出；默认 Ctrl+D 一秒内连续两次确认退出。最初讨论的单次 Ctrl+C 挂起及后续双 Ctrl+C 提议均被此决定取代。版本仍为 2.4.0，保持此前全部改动合入 main、删除本任务 codex 分支的要求。

## 实现核对
- root 移除 Ctrl+C 的全局 Quit 拦截与回退；agent 取消中重试 CancelTurn，minimal idle 不升级为 Quit。
- Quit 默认 Ctrl+D，非 VS Code 保留 Ctrl+Q alias。复用 PendingAction 一秒确认；默认半页向下采用既有 VS Code 的大写 D，避免绑定冲突。
- 全局退出键先于局部控件进入确认；第一次仅设置确认，第二次进入原 begin_exit_suspension；已有 exit operation 时重复退出键只合并。
- Welcome 保留显式菜单/拒绝信任等入口；Ctrl+C 本身不 Quit。Dashboard peek Ctrl+C 交给原回复编辑器；Replay Ctrl+C 不退出，Ctrl+D 在同一一秒窗口内双按才关闭本地播放器。
- 更新原 root/child/grandchild、Skill 冷恢复和 minimal 真实终端夹具为双 Ctrl+D。既有 stream cancel / resend PTY 用于验证单 Ctrl+C 仍取消且可继续发送。

## 验证方法与缓存边界
Atlas project(open) 与 scoped search 确认 AgentView::handle_agent_action；逐项阅读 root 输入、PendingAction、默认 registry、Dashboard 回复及 Replay 路由，并结合已归档挂起/取消契约核对。

此前隔离 index 快照与主工作区共享 target，Cargo 曾把旧的 4244-test Shell 可执行文件报告为 fresh；此结果没有运行，也不计为本次完整合并验证。为保证后续证据来自完整工作区，定向清理 shell/tools/tool-types/sampling-types/pager 旧产物（10.6 GiB），保留其他共享依赖后重编。固定 Rust/Rustdoc 1.93.1、两编译任务、16 MiB 测试栈、禁用 incremental/dev/test debug。

以下追加最终实际结果；未运行的平台或真实 provider 互操作不计为通过。

## 首轮定向反馈

重编成功后 Pager `ctrl_` 子集为 154 passed / 5 failed（`/tmp/grow-v2-4-0-exit132-pager-focus.log`）。三项失败暴露 Welcome 在 handle_global_action 中仍跳过确认；另外两项暴露移除全局 Ctrl+C 拦截时连同选区/闲置 child 历史的局部消费一起移除。修复为仅移除退出升级，保留原局部消费并明确清除旧确认；Welcome 键盘 Quit 统一只经过 registry 确认，显式菜单保持原行为。保留失败日志，待重编复测，不放宽产品断言。

Pager133 全量为 7503 passed / 1 failed / 12 ignored（`/tmp/grow-v2-4-0-exit133-pager-full.log`）。唯一失败是旧测试要求 Ctrl+Q 被行为切换警告吞掉；新契约明确注册的 global Quit 在局部确认前处理。按新边界拆分断言：Ctrl+C/Enter/Esc 等仍作本地行为决定；默认 Ctrl+D 及已注册 Quit alias 第一次只确认、第二次 Quit，期间不切行为且保留草稿和警告。不改生产逻辑、不移除该场景，重编后复测。

## 最终结果

- 完整工作区重新编译的 tool-types 123 passed、sampling-types 308 passed、Shell 4254 passed / 0 failed / 5 ignored；后者包含完整 feature graph 的 testkit 用例，不使用先前隔离快照产物。日志 `/tmp/grow-v2-4-0-exit132-{tool_types,sampling_types,shell}.log`。
- Pager `ctrl_` 子集 159 passed；修正上述 Welcome/局部消费后，最终 Pager 全量 **7504 passed / 0 failed / 12 ignored**。包括运行/取消中/空闲、草稿清理与保留、Welcome gate/picker、Dashboard、minimal、scrollback、child、确认到期/中断、挂起重复输入、unconfirmed retry/force、Replay 和行为切换警告的边界。日志 `/tmp/grow-v2-4-0-exit134-pager-full.log`。
- 实际 CLI 编译成功，`grow --version` 为 `grow 2.4.0 (2e69e5db)`；该 hash 为编译时 HEAD。最后仅调整测试诊断、旧夹具环境与文档，没有修改该二进制对应生产逻辑。日志 `/tmp/grow-v2-4-0-exit133-cli.log`。
- 真正 PTY 进程：`ctrl_d_` persistence 4 passed（运行命令中断与单次结算、不重放、root/child/grandchild 冷恢复、普通及 minimal 的原始 Skill 正文/身份恢复）；日志 `/tmp/grow-v2-4-0-exit136-pty-persistence.log`。
- 真正 PTY 进程：`minimal_ctrl_d_confirms_and_quits` 1 passed（重复 Ctrl+C 不退出，首个 Ctrl+D 显示确认，第二次退出）；日志 `/tmp/grow-v2-4-0-exit135-pty-minimal.log`。
- 真正 PTY 进程：`ctrl_c_cancel_during_stream_recovers_cleanly` 1 passed（取消标记恰好一次、下一条 prompt 正常执行、收到并处理 cancel）；日志 `/tmp/grow-v2-4-0-exit137-pty-queue.log`。旧夹具首次运行停在无模型配置页，未进入目标场景；补齐隔离 mock config、git 项目和 trust 后通过，没有放宽取消断言。

## PTY 首轮失败的保留边界

134 轮 persistence 为 3 passed / 1 failed，minimal Skill 在挂起前未找到预期的 `Input::Consumed` Skill 项；未修改生产逻辑或放宽快照断言。给该断言追加完整 Timeline 诊断后，单项 135 轮通过，完整 136 轮 4/4 通过，未复现首次失败。当前证据不能解释首次失败根因，保留原日志 `/tmp/grow-v2-4-0-exit134-pty_e2e_persistence.log`，不将复测通过表述为已经修复一个已定位的运行时问题。

## 场景与证据映射

| 契约场景 | 验证 |
| --- | --- |
| Ctrl+C 局部清空、取消、重试且空闲不退出 | Pager root/agent/Dashboard 单元测试；流式取消 PTY；minimal PTY |
| 首次 Ctrl+D 保留输入与执行，二次确认 | root confirmation/draft/release 测试、行为切换警告测试；minimal PTY |
| 到期、其他键或 Ctrl+C 中断确认 | root PendingAction 与 Replay 定向测试 |
| 挂起期间重复 Quit、失败重试与显式强退 | root/dispatch 既有挂起测试，经 Pager 全量通过 |
| 子视图与多 owned root | root/dispatch 既有 ownership 测试；嵌套 root/child/grandchild PTY |
| 外部命令中断、恢复不重放及 Skill 正文身份保留 | persistence 4 项 PTY 与 Shell 全量 |
| Replay 双按本地退出且 Ctrl+C 不退出 | replay_cmd::tree 测试；无执行能力边界的既有回归 |

未进行新的真实 provider、Windows/Linux PTY 或跨平台发布矩阵；这些结果不替代实际发布构建。本次只改变退出手势，保留既有挂起、命令中断和恢复架构。
