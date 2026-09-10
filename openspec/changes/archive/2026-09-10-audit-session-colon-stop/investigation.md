# Session 冒号停顿审计

session：`01a08906-7afd-70e2-af4e-5ff9ea4db84c`，项目 ScriptOS。结论以 2026-09-10 当前 Grow 源码 `162ba0ba`（Cargo 2.1.6）为准：已有修复没有完整覆盖这次现象。已确认一个仍存在的请求投影缺口；不能把所有停顿都归因于它。

本机 `grow --version` 返回 `grow 2.1.5 (155780e2)`。这证明 PATH 中安装的版本，不单独证明所有已启动进程的构建身份。仓库 2.1.6 改动包含在 `f8652cb6`，本次未安装或替换二进制。

## 实际发生了什么

时间均为 2026-09-10 上海时间。seq 从零开始，Timeline 文件行号为 seq + 1。

| 时间 | seq | 事实 |
| --- | --- | --- |
| 19:05:52 | 47021 | 最近一次压缩完成，mode 为 foreground。后面的三次停顿之间没有压缩。 |
| 19:30:12 | 49574 | 从 GLM 的 Chat Completions 路由切换到 `qwen3.8-flash` 的 Messages 路由。 |
| 19:31:35 | 49584–49591 | 首次“继续收尾。先处理 …：”后正常结束，整个 turn 没有工具调用。 |
| 19:32:17 | 49601–49613 | Qwen 返回 run_terminal_command，Grow 执行成功，Step continued。 |
| 19:32:59 | 49617–49624 | 截图上方“继续。先看 … 硬编码：”，Turn 正常结束，耗时 53893 ms。 |
| 19:34:09–14 | 49636–49650 | Goal 被清除，read_file 执行后前台 owner 在 Step 边界结束，Turn cancelled。 |
| 19:35:10 | 49660–49667 | 截图下方“继续。先查 … 记载：”，Turn 正常结束，耗时 49578 ms，本 turn 零工具调用。 |

三次只输出行动预告的响应都为 HTTP 200，evidence 未截断。原始流包含 thinking、text、完整 block_stop，随后是：

```json
{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{}}
{"type":"message_stop"}
```

上面省略 usage 的具体值以便看清终态。三次实际 output_tokens 分别为 4795、1119、2663，包含推理消耗；不能用 UI 文本长度推断 token 数。请求 max_tokens 为 131072，没有 `max_tokens` stop，也没有 `tool_use` block、丢失终态或采样超时。冒号是正文最后一个字符，Grow 没有在冒号上截断后续内容。

两次截图停顿的 request_id：`431f4400-bd2e-492e-b84b-f21d9e749a11`、`25b83aa5-0620-43f0-a17d-458a83b95c58`。Stop Hook 均为 allow_stop；当前 Shell 的 `turn/mod.rs` 在无工具、无待处理插话或完成通知时仍返回 `TurnOutcome::Completed`（约 1749–1795 行）。

## 另一个确定存在的问题：工具调用和结果被切开

两个停顿前的实际请求都配置了 23 个工具。但最新一次工具往返的 wire 不完整：

| 请求 observation seq | wire 中的 tool_result | 匹配 tool_use |
| --- | --- | --- |
| 49616 | `toolu_89200721c4134129bce8ec2b` | 无 |
| 49659 | `toolu_42b06e3ffe954dcba330f8d8` | 无 |

Timeline 中对应 assistant.tool_calls 仍存在（seq 49603、49639），工具也确实执行了。缺失发生在下一请求的投影阶段。

当前实现链条：

1. `sampler/src/stream/messages.rs` 约 696 行：thinking 的 signature 为空时，清除整段 native continuation，同时保留中性 assistant/tool 事实。这次原始流的 signature_delta 确实给了空字符串。
2. `chat-state/src/actor/mutations.rs::push_response_durably` 约 174–183 行：缺失 native 时将 portable prefix 重置到当前 Surface 末尾。此时 assistant 已提交，tool_result 尚未产生。
3. 工具随后执行，tool_result 追加在 prefix 之外。
4. `sampling-types/src/conversation.rs::request_segments` 约 1594 行：只对 prefix 内的消息调用 portable projector。`project_portable_history` 约 1497 行在这个切片内找不到结果，删除 assistant 的 tool call；prefix 外的 tool_result 仍作为当前协议消息发送。

所以 durable 事实是完整的，生成请求却留下孤立结果。2.1.5 `155780e2` 到当前 HEAD 的 diff 没有修正上述切点与配对逻辑。附带的 `projection_probe.rs` 构造这个切点，直接调用当前构建的 `build_messages_request`，用完整前缀和完整后缀作为对照；结果见 verification.md。

`model-sampling` 的 unsigned thinking 场景要求保留可见工具事实，`session-timeline` 要求可接受的上下文投影。已有 `proxy_thinking_signature_can_arrive_late_or_remain_absent` 测试只检查 Sampler 输出中的工具仍存在，没有覆盖工具执行后再构建下一请求，因此无法证明这条跨层路径正确。

## 为什么 2.1.6 已有修复不能直接覆盖

`preserve-async-compaction-task-continuity` 修复的是异步压缩在同一 turn 的两个 Step 之间发布后，追加一次有持久化来源的 AutoContinue。当前 `turn/mod.rs:1062` 明确要求 `async_compaction_applied && next_step_index() > 0`。这次最近压缩是更早的 foreground compaction，两次停止处不满足条件。摘要范围提示可能改善未来压缩后的输入，但无法据此认定此 session 已被修好。

`unify-sampling-attempt-recovery` 修复缺少终态、idle timeout、可重试传输故障等；提交 `80611943` 对应完整但非法的工具参数恢复。这次三次响应均有合法 end_turn，也没有非法工具 JSON，不能进入这些恢复分支。

Goal 在 seq 47002 已被用户暂停，behavior 为 Normal；seq 47019 保持此状态。截图中 Goal 命令失败、清除以及中间那次 cancelled 属于另外的控制过程，不能解释两次 completed/end_turn。清除 Goal 后仍出现同样停顿，也不支持“只是 Goal 状态卡住”的解释。

## 仍然不能证明的部分

首个停顿（request seq 49583）没有原生 tool_use/tool_result，旧工具往返均已转为 Historical tool exchange 文本，因此不能把三次停顿全部解释成孤立 tool_result 导致。长历史、模型切换、工具历史文本化或供应商行为都可能影响模型输出，但本次没有线上 A/B，也没有供应商内部证据。

能够确定的是：Grow 收到服务端合法结束；当前版本仍有会丢失工具调用配对的可复现投影问题。修复配对后，需要以 Messages 路由的真实下一请求及后续行为验证是否减少提前结束；不能承诺升级 2.1.6 就消失，也不应依据冒号或短句强制重采样。

## 证据位置

原始数据：`~/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2FScriptOS/01a08906-7afd-70e2-af4e-5ff9ea4db84c/timeline.jsonl` 及同目录 `artifacts/sampling/<blake3>.bin`。通过各 observation 的有序 chunks 拼接，核对总字节数，按 SSE `data:` 行解析；兼容冒号之后没有空格的原始格式。

相关归档：`2026-09-10-preserve-async-compaction-task-continuity`、`2026-09-10-unify-sampling-attempt-recovery`；对照测试 `sampler/src/stream/messages_tests.rs::proxy_thinking_signature_can_arrive_late_or_remain_absent`。Atlas 提供了部分局部导航，但对源码实际存在的部分符号返回空结果，本报告的具体控制流以直接源码及实际 wire 为准，未把查询为空当成不存在。
