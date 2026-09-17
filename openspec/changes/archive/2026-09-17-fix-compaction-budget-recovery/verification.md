# 根因与验证

## 证据范围

用户暂时无法提供失败 session ID 或原始错误，只记得“压缩失败，建议 /new”。这段文案来自 `suppress_auto_compaction` 的 Other 分支，不能据此断言某一次事件命中了 Size。以下结论来自当前生产链路和可重复的本地 provider 故障注入；不声称回放了该次线上会话，也未修改用户账本。

## 已确认的失败链

1. `prepare_compaction` 原先把输入窗口全部交给 Sideband，输出 policy 为 None。`ShellCompactionSampler` 的审计请求和 `generate_session_compact` 的实际请求均未设置输出上限；SamplingClient 随后补主模型 output_limit，Messages 未设置时甚至使用 128000 的默认上限。输入靠近窗口时，输入与输出的组合可超出 provider 允许的总窗口，本地仅输入准入无法阻止。
2. 固定 32768 reserve 的 Fitted 降级不随小窗口缩放；随后 Simplified 删除工具结果、参数和附件。两种裁剪都仍按原 `range_plan.target` 发布，未进入摘要的来源也被遮蔽。
3. Size 失败及提交后仍超窗留下 SUPPRESS_STICKY。三个入口 `check_auto_compact_needed`、`check_preflight_overflow`、`compaction_window_on_error` 都被其挡住，普通回合虽然仍可采样，溢出恢复却无法继续。手动 /compact 原本仍可用，因此这不是所有 session 命令或持久化账本都失效。
4. Recap 的 pop_trailing_tool_run 删除完整工具尾部，独立于压缩的降级问题。

## 修复边界

- 摘要上限取窗口 1/8、最多 32768、且不超过显式模型输出上限；额外留 5% 输入估算余量，System 和摘要指令占用一起计算。Sideband manifest 与三个实际 wire 都设置相同输出上限。
- 预算适配只收窄完整旧历史前缀。明确 overflow 最多再收窄两次；暂态重试保持同一冻结请求。选中的 Surface IDs、source_tokens 和成功 replacement target 同步，未纳入来源的消息保持原身份与内容。
- non-verbatim 使用已有 portable 投影，保留调用参数、结果和附件。无完整可用范围则不发送、不替换；不以截断单个超大工具结果逃避预算。
- Size 和提交后仍超窗只抑制当前 durable turn，下个真实 TurnStarted 清除；同回合继续保持有界停止，持久化错误仍 fail closed。
- Recap 的快路径尊重 reasoning 参数，预算路径继续去 reasoning；裁剪前后用已有 portable 投影保证工具配对。

## 回归场景

- `compaction_bounded_requests_preserve_evidence_and_target_on_all_backends`：三个 backend × 两种 verbatim 设置；100k 窗口、131072 主模型输出上限、六个完整工具交换，完整原选区超预算；注入首个 provider overflow，验证初次预算缩区及第二次缩区均保留执行结果、参数、文本附件和图片。检查每次输出 cap=12500、输入<=82500、成功 selected IDs=Summary target、其余历史逐项保留身份和内容。Messages 的一个组合额外注入 503，验证重试 body 与 manifest 完全一致。
- `compaction_size_failure_allows_the_next_user_turn_to_recover`：第一次自动摘要被输入超长拒绝，不可再拆的来源不重复发送，主历史不变；同 session 的下一真实 turn 再次压缩并完成正常采样。
- `pre_prune_error_fails_open_to_summary`：保留旧测试的 prune 持久化冲突分支；原 Phase B 使用单个 50k 工具结果和 40k 模型窗口，却让 mock 接受请求。现在明确验证该不可分割输入安全拒绝、零 provider 调用、原身份和内容保留；再以同总量的两个完整交换验证受抑制时手动 /compact 仍成功，未选中的另一半原文保留。没有删除或缩小原失败场景。
- `context_window_exceeded_converged_over_window_fails_turn`：继续保留两次请求上界与 Completed 事务，更新为 turn-scoped suppression。
- 新增四个纯选区回归：完整并行工具结果与 reasoning 边界、5000 最小来源及超大不可分割组、过期 identity 拒绝、未完成调用/部分结果/reasoning 尾部不进入 target。
- Recap 完整/部分批次、附件和超预算证据回归；边界测试保持精确 `<=` 断言，fixture 改为 User→Reasoning→配对调用/结果，而非 User 前游离 reasoning。

## 验证执行

- 初次 shell 编译发现新增 sampler 参数误加到 observer 构造器，已纠正。
- 新增完整请求回归在 HEAD 旧生产实现上失败：`initial selection must fit the full request budget`。四个生产文件使用临时快照保护并在 finally 中恢复，之后修复版回归通过。
- 复核另发现原候选 end 被无条件当作闭合边界；新增 unfinished-evidence 用例先失败（target 包含未完成工具 Assistant），随后以局部 pending call 集合校验修复，确保未执行部分保持 live。
- chat-state `compaction_utils::tests::`：首次 135 passed；补充未完成尾部边界后 136 passed。
- shell `compact`：首次 135 passed / 1 failed（上述不合法窗口 mock），保留失败场景并补安全边界后 136 passed；完成未完成尾部修复后再次 136 passed。
- shell `recap`：初次 62 passed / 1 failed（上述游离 reasoning fixture），修正后 63 passed。
- `rustfmt --edition 2024 --check`：本轮修改的六个生产/工具 Rust 文件通过；集成测试只格式化新增函数，未批量改动旧测试排版。`git diff --check` 通过。
- `openspec validate --all --strict --no-interactive`：归档前最新 19/19 通过（包含并行任务新建的 coordination change）。
- `openspec archive fix-compaction-budget-recovery --yes`：归档成功，四项新增契约合入主规范。
- 归档后 `openspec validate --all --strict --no-interactive`：18 passed / 0 failed；`openspec validate --archived --no-interactive`：340 passed / 0 failed。

Mac 链接仍有既有 `__eh_frame` 超过 16 MB 警告，测试二进制成功链接。不安装或替换运行中的 Grow，不提交 Git。Atlas scoped 查询有陈旧/空结果，源码行与真实调用以当前文件核对。

## 联合架构核对

与并行 `fix-coordination-inquiry-tool-evidence` 任务核对：冻结事实由 ChatState 提供，已有 sampling-types portable pairing 负责独立请求的工具协议投影，预算与替换归各消费者所有。压缩默认 verbatim 仍保留完整选区来源，non-verbatim 与 recap/问答复用 portable；范围 helper 只判断可替换的完整因果边界，不新增投影框架。权限 PermissionJudgment 以真实用户授权证据为来源，不能与问答上下文混用或从错误问答反推权限丢失。coordination 代码由另一任务独立修复，本轮不修改；memory flush 仍用丢工具结果的旧摘要输入，已登记独立债务，不在本轮实施。
