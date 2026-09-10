# Grow 会话 `01a08906-7afd-70e2-af4e-5ff9ea4db84c` 调查记录

## 范围与方法

只读扫描了该会话的 `summary.json`、`updates.jsonl` 和 `timeline.jsonl`。以 timeline 中 `tool/state=started,name=get_goal` 与同一 `call_id` 的 `tool/state=completed` 为计数和耗时边界；以相邻的 `messages` 事件和 tool step 判断 turn 边界。没有读取或复述业务正文，也没有修改会话目录。

## 结果

- `summary.json`：会话创建于 `2026-09-10T01:54:58Z`，更新至 `2026-09-10T08:43:14Z`，共 14,226 条消息。
- `updates.jsonl` 中出现 26 个 distinct `get_goal` 调用；每个都有对应的 `Goal: read status` 展示更新。
- `timeline.jsonl` 中 26 个调用均有 started/completed，结果均为 `success`；没有发现失败、取消或悬挂调用。
- 23 次发生在 step 0；另有 1 次发生在 step 6、1 次发生在 step 17、1 次发生在 step 47（后 3 次属于已有 turn 的后续步骤；其余 step 0 统计按实际记录保留，不推断一定是新的用户 turn）。
- 调用前的可见助手意图高度一致：先做 Goal/C4 完成审计，再读取 Goal 状态，然后继续读取文件、grep 或推进下一切片。没有证据表明 UI 刷新或定时轮询触发这些调用。

## 耗时分布

timeline completion 事件报告的 `duration_ms`：

`[70, 77, 88, 112, 116, 122, 126, 135, 137, 137, 149, 155, 160, 165, 168, 170, 170, 173, 180, 186, 189, 189, 212, 257, 265, 340]`

- 最小 70 ms，最大 340 ms，平均约 163.4 ms，中位数 162.5 ms。
- p90 约 259.4 ms；超过 250 ms 的慢样本为 257 ms、265 ms、340 ms，各 1 次。
- started 到 completed 的 timeline 时间戳差与报告 duration 基本一致，个别记录存在约几十毫秒的事件记录误差；以下结论使用 completion 事件自身的 `duration_ms`。

## 慢样本时间线与边界

- 257 ms：发生于 turn `13433619500474264085`，step 0。
- 265 ms：发生于 turn `11987603681973948638`，step 0。
- 340 ms：发生于 turn `12957114119708342066`，step 6。

每次调用附近都可观察到 `permission_requested` → `permission_resolved(wait_ms=0)`，随后才有 completed 事件。这个顺序说明工具展示至少包含权限事件和实际工具完成事件，但现有日志不能把总耗时可靠拆成“Goal 读取、hook、串行 batch 等待”各自耗时。特别是 `wait_ms=0` 只表示权限等待记录为零，不能推出没有排队或没有其他串行等待。

## 对实现与验证的约束

这些记录支持“重复的完成审计路径会在 turn 边界主动调用 `get_goal`，而不是 UI 定时查询”的观察。本次不继续拆解亚秒级样本内的各阶段耗时；工具展示 elapsed 不能代替这些阶段的测量。用户看到的长计时由下面的 UI 生命周期证据解释。

## UI 时间口径复核

对 `updates.jsonl` 逐个 call id 配对后，文件中每个 `get_goal` 只保留三条 UI 记录：最早的 `tool_call`（标题 `get_goal`）、`Goal: read status` 标题更新、以及带 permit 的后续 `tool_call_update`。没有带 `status=Completed` 的最终 ToolCallUpdate，因此不能从这份持久化 updates 流直接得到“UI 行结束时间”。三条记录的 agentTimestamp 差值为 20–77 ms（中位数 32 ms），这不是工具真实执行时长。

`timeline.jsonl` 才有明确的工具 started/completed 边界；其 `duration_ms` 为真实工具事件区间，全部低于 340 ms。UI 侧工具块的 elapsed 起点由 Pager `ToolCallBlock::start_timing` 在块进入 running 状态时设置 `Instant::now()`；完成时由各 block 的 `finish()` 以该 `started_at` 计算。`preparation.rs` 仅把 `ToolInput::GetGoal` 标题映射为 `Goal: read status`，没有计时逻辑。

最慢三个真实执行样本（UI 记录使用 updates 的 agentTimestamp；执行区间使用 timeline 的 at_ms）：

| 报告 duration | UI 首次显示 / `Goal: read status` | timeline started → completed | 所属 turn / step |
| ---: | --- | --- | --- |
| 340 ms | `2026-09-10T08:43:57.143Z` / 同时刻 | `08:43:57.143Z` → `08:43:57.469Z`（326 ms 事件差） | `12957114119708342066` / 6 |
| 265 ms | `2026-09-10T06:30:21.948Z` / 同时刻 | `06:30:21.948Z` → `06:30:22.218Z`（270 ms 事件差） | `11987603681973948638` / 0 |
| 257 ms | `2026-09-10T08:34:53.476Z` / 同时刻 | `08:34:53.476Z` → `08:34:53.739Z`（263 ms 事件差） | `13433619500474264085` / 0 |

这里的 `updates` 文件没有最终 completed UI 更新，原因已由源码核对确认：`shell/src/session/acp_conversion.rs::acp_tool_update` 原先只为 `UpdateGoal` 生成完成更新，`GetGoal` 与 `CreateGoal` 落入 wildcard `None`；`shell/src/session/actor/tool/result.rs::handle_tool_result` 只有在 `acp_tool_update` 返回 `Some` 时才发送 `ToolCallUpdate(Completed)`。因此 pager 的 `acp/tracker.rs::finish_turn` 只能在 turn 收尾统一 drain `pending_tools` 并调用 `scrollback.finish_running`，查询实际完成后仍可能显示 Running，直到整个 turn 结束。会话中 26 次调用均缺少最终 completed UI 事件，与该机制完全一致。

结论是：约 21 秒的可见 Running 时长是工具完成事件缺失后延迟到 turn 收尾的 UI 生命周期问题；不能把它当作 `get_goal` 的真实执行耗时。修复范围保持最小：为 `CreateGoal`、`GetGoal` 增加 ACP completed 分支，并保留现有 `UpdateGoal` 分支；不引入 Goal 查询队列分流或额外轮询。
