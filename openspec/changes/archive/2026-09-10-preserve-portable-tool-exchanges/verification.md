# Verification

## 证据与结论边界

原始会话为 `01a08906-7afd-70e2-af4e-5ff9ea4db84c`，见 [原始审计](../2026-09-10-audit-session-colon-stop/investigation.md)。它的 seq 49616、49659 请求缺少与最新 tool_result 对应的 tool_use。三次行动预告后都收到完整的合法 end_turn；第一次请求没有孤立结果，不能将全部语义停顿归因于配对缺口。

本变更从工作区 `5a248592` 开始验证，包版本为 2.1.6。修复请求结构和历史事实表达，不声明已证明 Qwen 或所有供应商线上模型不会提前完成。没有调用真实 provider、执行原始会话工具、改写用户会话或安装发布版本。

## 先失败，再修复

- `portable_boundary_preserves_multi_tool_pairs_at_every_cut` 在旧实现失败：跨切点请求只包含 call_a/call_b 的两个结果，没有对应调用。
- `unsigned_thinking_tool_exchange_survives_the_next_request` 在旧实现失败：真实 Sampler → ChatState → TodoWriteTool → 下一 Messages 请求只保留 unsigned_call 的 tool_result，调用数量为 0。
- 保留结构化历史后，原有 `portable_history_is_allowlisted_for_all_wire_backends` 检出工具附件的淘汰文本被编码器忽略；保留原断言并修复三个 backend 的 Text 分支，没有删掉失败场景。

## 场景映射

| 契约场景 | 验证 |
| --- | --- |
| unsigned response 后追加结果 | Shell 真实 Turn 回归检查两次请求、一次工具完成事实、合法冒号结尾最终响应，无多余采样；ChatState 三 backend durable admission 回归检查配对、原始 reasoning 留存。 |
| 多工具跨 prefix | 枚举全部 7 个切点 × 三 backend，两个结果逆序完成，调用/结果各一次。 |
| token 估算和来源证据 | ChatState 三 backend 校验闭合后的 source_projection、与整体 portable 相同的估算、图片计数。 |
| 旧历史恢复及模型切换 | 三 backend 恢复历史、六方向 endpoint switch 及切回原路由，保留工具事实且不复活 native。 |
| 无效与歧义历史 | 保留悬空/孤立回归，补充重复调用 ID、重复结果、缺名字/ID、非法/非对象 JSON 和被用户消息隔开的结果。 |
| 有效 native | 闭合旧工具结果止于新 signed span，旧/新调用各一次，原 thinking/signature 保留一次；既有 native 测试保持。 |
| 图片及替换文本 | 三协议结构化历史保留图片和淘汰说明；live/portable 只剩替换文本时也按原顺序保留一次。 |
| partial / async compaction | 真实 between-step 摘要发布后下一请求保留 async-todo 调用/结果，工具完成一次，AutoContinue 仍在结果之后，turn 完成一次。 |
| 合法 end_turn | unsigned Turn 的最后响应故意以冒号结束，只有一次工具 Step 和一次最终 Step，无文本启发式重试。 |

## 命令与结果

- 旧实现定向复现：上述 sampling-types 与 Shell 测试均按预期失败，分别显示调用数量 0/预期 2 和 0/预期 1。
- `RUST_MIN_STACK=16777216 cargo test --locked --lib -p shell -j 2 -- truncation_recovery_tests compaction_pre_prune_tests --test-threads=4`：46 passed，0 failed，3746 filtered out。链接器保留既有大型 debug 测试二进制的 `__eh_frame` 警告，测试通过。
- `RUST_MIN_STACK=16777216 cargo test --locked --lib -p chat-state -p sampling-types -p sampler -j 2 -- --test-threads=4`：ChatState 474 passed / 1 个既有 ignored；Sampler 240 passed；sampling-types 274 passed。共 988 passed，0 failed。结合 Shell 为 1034 passed。
- `git diff --check` 通过。只对本次 Rust 改动区域应用 rustfmt，没有全仓格式改写。
- 磁盘：开始构建时可用约 15 GiB；最低约 7.2 GiB。只用现有 target、限制编译并发为 2，未构建 release/CLI；收尾删除本次 Shell 增量目录及 256 个已完成链接的中间对象，保留测试可执行文件，回收约 4 GiB，最终可用约 11 GiB。没有清理用户会话或其他项目。
- 归档前 `openspec validate --all --strict --no-interactive`：19 passed；`openspec archive preserve-portable-tool-exchanges --yes` 合入 3 条 Requirement；归档后同一全量校验 18 passed，`openspec validate --archived --strict --no-interactive` 311 passed，均 0 failed。

## Backlog 复核

本次闭环顶部两项同一数据链的已复现问题：无签名回退拆开往返、压缩/恢复/切换后的完整工具历史被文本化。工具附件 Text 丢失是本次回归发现的直接相邻问题，一并修复。

其余条目按现有证据保留边界：signature delta 多片段语义没有真实复现；MCP Elicitation、Worktree 与 Status line 是长期或搁置产品方向；Pager 大部分是模块/测试架构债务；Workflow checkpoint/tombstone、FD/trust capability、用量持久化、失败证据、实时补传、Summary 只读修复、日志/资产资源生命周期需要各自契约和故障验证。它们不能从本次测试得出“已修复”结论，也不以修复一次 session 请求投影为由合入跨领域重构。backlog 的已完成记录不重复实施。
