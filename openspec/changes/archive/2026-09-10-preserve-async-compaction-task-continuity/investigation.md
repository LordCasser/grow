# 异步压缩后停止的证据

目标 session：`01a08910-219a-78f1-a45c-98b448764a05`，工作目录 skynet。只读检查原始会话，没有重放工具或向其发送消息。截图内 HTTP 方法约束属于被调查任务的内容，不是本次 Grow 修改指令。

## 事故时间线

以下 seq 为 Timeline 事件序号，文件行号为 seq + 1。时间为 UTC，上海时间加八小时。

| seq | 时间（2026-09-10） | 事实 |
| --- | --- | --- |
| 12536–12546 | 09:08:16 | Step 17 调用 read_file 读取 design 对应节，工具成功，Step 以 continued 结束。 |
| 12547–12552 | 09:08:16–17 | summary、范围 replacement、completed、post_compact 全部成功；后台冻结输入的 high-water 为 12479。 |
| 12553–12555 | 09:08:17 | 同 turn Step 18 开始，request `7e09aec7-1d0b-4246-a7ca-f11d98a09c28` 发出，23 个工具定义仍在。 |
| 12556–12558 | 09:08:21 | HTTP 200，完整 SSE 仅产生“改写 platform design 的对应节：”，usage 为 135699 input / 9 output tokens，finish_reason=stop，随后 [DONE]，没有 tool call。 |
| 12560–12563 | 09:08:21 | Step completed、Stop Hook allow_stop、Turn completed/end_turn；耗时 264680ms，对应截图 4m24s。 |
| 12566–12573 | 09:10:33 | 用户另发“继续”，新 turn 正常开始，说明原 turn 确已结束，不是 UI 卡死。 |

证据位于 `~/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fskynet/<session>/timeline.jsonl` 及其 `artifacts/sampling/`。原始请求 515862 bytes、原始响应 2840 bytes，由 observation 的 content-addressed chunks 读取。本文仅保留必要摘要，不复制完整项目上下文或隐私内容。

## 输入变化与成因

- 压缩前最后一个请求（seq 12532）有 102 条带 tool_calls 的 assistant 消息、113 条 tool 消息；压缩后的 seq 12555 没有 tool_calls/tool role，共 128 条 Historical tool exchange 文本。
- `finish_surface_replacement` 重置 native epoch，`portable_prefix_len` 覆盖全 Surface；`project_portable_history` 因而文本化全部保留工具往返。未选中 tail 的 Timeline identity 没有丢失，wire 表达发生变化。
- 旧片段摘要的 Pending Tasks 是“None outstanding”，Optional Next Step 是“No next step should be started without the user”，对应早前已完成的 SOAR 工作。较新的 HTTP 方法任务及刚结束的 read_file 仍在摘要之后，不属于摘要范围。这些旧结论本身不是摘要造假。
- 当前摘要载体仅称会话因上下文不足而继续；它没有明确旧任务状态与后续请求的覆盖关系。尾部也没有 AutoContinue，仅剩最近工具输出文本。后续模型生成准备动作的短句后合法停止，Shell 按正常完成接纳。

因此，**直接停止原因是 provider 的合法 stop 加无工具调用**；**已证实的 Grow 架构缺口是压缩后的任务语义续接缺失**。旧完成摘要和工具协议文本化是可观察到的促成条件，无法从单个样本证明哪个条件必然导致模型 stop。不能归咎为后台取消、Token 上限、丢失 terminal、工具不可用或 UI 错判。

事故前最后的 Control（seq 10639，revision 489）为 Normal，没有 active Goal。既有 Goal idle continuation 因而不适用；LazinessDetector 是模型配置显式启用的 idle 分类器，注入仍受 active Goal 门控，并不是普通 turn 的通用自动续接。没有将这两个不同生命周期的机制扩展为本次修复。

## 方案取舍

采用有持久化来源的摘要范围说明与下一 Step 续接提示；保持 model/native 安全边界。没有通过关键词/冒号/回复长度猜测“模型想继续”，也不将每次压缩后的合法 stop 强制重采样。跨三协议的 portable 结构化工具历史需要单独设计；不在本次加入第二套 completion classifier 或恢复 actor。

Atlas scoped search 完成了局部符号定位；incoming call 查询报告 focus_closure budget_exhausted，不能将空 callers 视为全仓库无调用。实际两个 `background_compaction_boundary` 调用点已通过源码核对。独立低级模型 subagent 只读核对规范与 native 约束；最终判断以原始证据及当前代码为准。
