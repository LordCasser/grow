# 验证记录

## 故障证据与原因

只读检查本机 GameNow session `01a14e30-5417-7c61-b7c0-8b3412a9289d`，Timeline seq 1718/1737 均记录 HTTP 200、Chat `finish_reason: tool_calls` 后的 Serialization fatal error，列号 239。它们引用的原始 SSE usage 尾帧含 `prompt_tokens_details.audio_tokens: null` 以及多个 completion 可选明细 `null`。`#[serde(default)] u32` 只接受字段缺失时的默认值；显式 null 会失败，导致已经收到 finish 的候选也不能接纳。第二次用户输入触发同类尾帧，因此再次失败；不是读取截图的工具参数失败。

原始证据摘要（不复制工具参数或会话正文）：

| Timeline seq | Response artifact BLAKE3 | input | output | cache read | reasoning |
| --- | --- | --- | --- | --- | --- |
| 1718 | `239cd8feaf933992a7775d770306d79861d750f1b6e08ed3e4ff41d12673b2cc` | 154451 | 157 | 0 | 82 |
| 1737 | `a34899b1cfcd2539b4e34f503016c340f7147d58a1692128b76fc249dc7d8c5f` | 154469 | 157 | 147456 | 74 |

## 验证结果

- 修复前，`cargo test --locked --offline -p sampling-types --lib types::tests::chat_ -- --nocapture`：新增的两个 nullable 测试均以 `invalid type: null, expected u32` 失败，已有 finish reason 测试通过。
- 修复后，`CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo test --locked --offline -p sampling-types -p sampler --lib -- --test-threads=4`：sampler 255、sampling-types 303，全通过。新增矩阵覆盖 prompt/completion 明细的缺失、null、零、正整数、u32 上限，并拒绝负数、字符串、浮点、bool、溢出和缺失/null 必需总量。HTTP mock 经过真实 SSE decoder、Chat stream 和 sampler actor，确认 tool-call response 完成，且仅一次 `AttemptUsage::Known` 与响应完整用量一致。
- 将上述两份原始响应按字节复制到临时目录，确认副本与源文件相同；用临时 Rust integration test 经 `ChatCompletionChunk` → `stream_chat_completions` → `collect_response` 离线重放：各 160 帧均完成，一个工具调用，总输入/输出、缓存读及 reasoning 与上表一致。测试通过后删除临时测试源码和证据副本；原 session 文件没有改写，没有重发 provider 请求或执行历史工具。
- `cargo fmt -p sampling-types -p sampler -- --check`、`git diff --check` 通过。
- `openspec validate fix-null-chat-usage-details --strict --no-interactive` 通过。
- 第一次 `openspec validate --all --strict --no-interactive`：19 passed / 1 failed；并行产生的独立 `evolve-task-planning` change 尚无 delta。本次 change 和全部 14 项主规范均通过，未修改该独立 change。

## 对抗审查与限制

核对了全仓库这些明细类型的构造/读取点；只有 Chat `TokenUsage` reasoning 映射需要跟随 Option 更新，其他内部账本与 Responses/Messages 未改。拒绝路径仍通过 serde 严格整数校验，不增加跳过事件或重试分支。Luna 子 agent 的只读对抗审查发现新增测试未解构 accounted API 返回元组；已修复并通过上述完整 suite。

本机正在运行的安装版为 `grow 2.3.1 (7b7d56a)`，代码修复不会热替换该进程。当前源码 HEAD 与安装版相差 184 个文件，本次是否替换本机可执行文件另由用户选择；没有擅自重启会话或改写安装文件。历史失败 attempt 保持原有不可变证据，不回填过去的用量账本。

## 归档校验

已归档为 `2026-10-18-fix-null-chat-usage-details`，新增要求已合入主规范；四个场景分别由 nullable wire 矩阵、严格拒绝矩阵及 accounted HTTP SSE 回归覆盖。归档后全量校验为 18 passed / 1 failed，唯一失败仍是独立 `evolve-task-planning` 缺少 delta。archive 初次校验在本 change 的归档收尾 task 尚未勾选时提示一项 incomplete；完成收尾记录后重新校验结果如下。

最终归档校验：`openspec validate --archived --no-interactive` 为 577 passed / 0 failed；再次全量严格校验为 18 passed / 1 failed（仍仅独立 evolve-task-planning）。本次创建的 Cargo target 原先不存在，验证后清理该编译缓存。
