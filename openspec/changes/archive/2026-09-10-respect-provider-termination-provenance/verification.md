# 验证记录

实现基线为 `77ea7978` 工作区。当前变更撤下强制 FinishTurn，分离 provider 原始终止、候选接纳和宿主生命周期；未改写用户历史会话、提交版本或替换已安装二进制。

## 证据与边界

原始 session 的三次定位响应确实在 HTTP body 中含 `end_turn` 和 `message_stop`；详见先前来源审计。它们不能证明任务完成，但也不能当成 Grow 的异常补全。本次尊重这种自然终止，不宣称消除了 provider 自身的提前结束行为。

ProviderTerminal 保存 Chat finish_reason、Messages stop_reason/stop_sequence、Responses event/status/incomplete reason。中性 StopReason 不再被工具存在覆写；拒绝与完整工具同时返回时，保留调用和明确未执行的配对结果，不执行业务动作。

Request::Completed 保存候选所属 attempt 和 native terminal。TurnTerminal.source 的 Provider 引用必须属于本 Turn 的已完成、有原生终止证据的 request；宿主错误/控制、用户取消、恢复独立归属。历史来源缺失保持 Unknown。

## 验证映射

| 场景 | 验证 |
| --- | --- |
| 三 API 自然结束且正文为中文冒号预告 | `provider_completion_preserves_native_stops_and_tool_authority_on_all_backends`：不含 FinishTurn，只请求一次，原始字段和 request source 匹配 |
| 三 API 原生正常结束且带完整工具 | 同一矩阵：执行一次工具，再请求最终回答，第一响应原因不改写 |
| 三 API 拒绝且带完整工具 | 同一矩阵：一次请求、Refusal、零业务执行、存在未执行的配对结果 |
| 原生终止后候选被拒收 | `malformed_completed_tool_arguments_recover_with_bounded_accounted_attempts`：真实 HTTP 返回 completed，无效参数拒收后 response evidence 仍保留 terminal，恢复/取消/结算失败等模式均检查 |
| 无原生终止/传输错误 | 既有 missing terminal、断流、协议冲突测试及 evidence ACK 测试；HTTP 错误保留 raw body，native terminal 为空 |
| 切换端点与 API | `provider_completion_keeps_original_request_source_across_endpoint_switch`：旧 Messages 响应阻塞期间提交 Responses 路由，旧回合保留原 request 来源，下一回合才使用新端点 |
| 新输入、取消、结构化输出与 Goal 预算 | provider_completion 其余三个回归，保留既有独立准入/终止契约 |
| 历史与恢复 | `provider_terminal_source_requires_this_turns_completed_request`：正常重放、拒绝跨 Turn 引用/无原生终止/缺 attempt，Recovery 来源及旧 Unknown |
| 协调进程 mock | Chat/Responses × 普通前台/Sideband/工具调用/工具后最终回复：8 个真实 HTTP 夹具场景；不再生成 FinishTurn，AST 语法验证通过 |

## 测试修正

- 更新旧测试中“工具覆盖终止原因”的预期，仍检查完整调用和原始终止均保留。
- 移除压缩测试对已经删除的 FinishTurn 第三条结果的索引，保留两条业务结果及裁剪/摘要断言。
- 新端点切换测试使用直接 handle_prompt 夹具，补上生产完成邮箱所负责的 Settling → Idle 释放后再提交第二回合。
- 全量初次运行暴露首次 Goal bootstrap 测试仍设 8 MiB 栈。已有生产 spawn.rs 在 debug 使用 32 MiB（release 8 MiB）；夹具同步 debug 条件，不更改生产栈，不跳过该场景。

## 限制与资源

验证覆盖本地 mock 协议和宿主状态机，未调用真实付费 LLM 端点，也未重新运行完整独立进程 release 测试组。普通自然终止不保证业务任务已经完成；等待依赖及 Goal 状态仍由既有契约负责。

工具关联 ID 清洗碰撞、Responses phase 的 portable 保留按独立 backlog 处理，未混入本次变更。v2.1.6 changelog 与旧 FinishTurn 归档仅作历史记录；并行发布夹具提案已注明新契约替代旧前提。

复用既有 target，构建使用 `-j2`，未新建 checkout 或复制历史 session。观察到磁盘最低约 5.5 GiB，之后其他工作释放了空间；未删除其他任务的缓存或产物。Shell 巨型单元测试二进制有既有 macOS `__eh_frame` 大小链接提示；测试可正常执行。

## 执行结果

- ChatState：476 passed，1 ignored（`/tmp/grow-provider-terminal-unit.log`）。
- sampler：240 passed；sampling-types：274 passed（`/tmp/grow-provider-terminal-sampling.log`）。
- 5 个 provider_completion 集成测试全部通过，其中含三 API 九场景矩阵和真实路由切换。
- Shell 全量：3794 passed，3 ignored，93.33 秒（`RUST_MIN_STACK=33554432 target/debug/deps/shell-04653512b14be20c --test-threads=4`；`/tmp/grow-provider-terminal-shell-all-final.log`）。
- 四个 crate 合计 4784 passed、4 ignored；忽略项沿用既有配置。
- `git diff --check` 通过；最终构建日志 `/tmp/grow-provider-terminal-shell-build-final.log`。
- 最终观察磁盘余量约 10 GiB；已删除本次 Python 导入产生的 45 KiB pycache，未删除用户会话、其他任务产物或目标缓存。

- OpenSpec 归档前 strict 全量：20 passed；归档成功，主规范增加 2 项、修改 1 项、删除强制完成要求 1 项。
- 归档后 strict 全量：19 passed。归档任务包含归档后复核，初次 archive audit 因该任务尚未勾选而报告 1 项未完成；复核完成后勾选，再执行 archive audit。
- 最终 archived strict 校验：315 passed、0 failed（`/tmp/grow-provider-terminal-spec-archived.log`）。
