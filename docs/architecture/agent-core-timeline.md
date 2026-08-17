# Agent Core Timeline Architecture

Grow 的会话核心以不可变 Timeline 为唯一事实源。模型上下文、用户 transcript、诊断视图与持久化缓存都是 Timeline 的投影；压缩、修剪、图片替换、回退和系统提示变化只能追加事件或切换投影，不能改写已接受的事件。

本文约束 `crates/codegen/chat-state`、`crates/codegen/shell/src/session` 与会话存储之间的边界。采样协议、foreground 调度、Behavior、权限和 Workflow 保持各自现有 owner，不进入 Timeline 数据模型。

## 分层与依赖方向

```text
Shell turn/runtime
        |
        v
Context assembly -----> Sampling
        |
        v
Surface projection
        |
        v
Timeline -------------> Timeline persistence
```

- Timeline 只拥有有序事件、事件身份、Surface fold 与结构校验，不依赖 shell、TUI、provider 或具体持久化后端。
- Context assembly 从 Surface 生成 `ConversationRequest`，可以对请求副本做临时的图片逐出等 wire 限制，但不得改变 Timeline。
- Shell 追加 turn、message、tool、governance 与 routing 事件；它不能直接替换历史数组。
- Timeline persistence 只追加已接受事件。投影检查点和 `chat_history` 类文件若存在，只是可删除、可重建的缓存。
- UI 更新与本地诊断是 Timeline 的消费者，不是恢复会话的第二事实源。

## 核心类型

每条事件具有单调连续的 `seq`。`seq` 在接受时由 Timeline 分配，调用方不能指定或复用。

```text
TimelineEvent {
  version,
  seq,
  kind,
}

SurfaceOp = Append | Replace { start, end, shadowed }
```

消息类事件携带一个或多个完整 `ConversationItem` 与 `SurfaceOp`。普通用户、assistant 与工具结果使用 `Append`；压缩和内容重写使用 `Replace`。非消息事件不能携带 `SurfaceOp`。

`Replace` 的 `start`/`end` 引用当前 Surface 节点身份，不是 `Vec` 下标。接受替换前必须验证：

1. 两端节点都仍在当前 Surface；
2. `start` 不晚于 `end`；
3. `shadowed` 恰好覆盖被遮蔽的 Surface 节点，无重复且全部早于新事件；
4. 工具结果内容重写只能替换一个 `ToolResult`，并保留 `tool_call_id` 与其他结构字段；
5. 事件验证失败时 Timeline、Surface 与持久化队列都不变化。

Timeline 接受的消息是完整原子记录。采样流的 token/delta 只属于 sampler 到实时 UI 的传输链，不进入 Timeline。

## 三种读取投影

- `surface()`：折叠 `Append` 与 `Replace` 后的当前模型消息序列，是上下文组装的唯一历史输入。
- `transcript()`：只读取 append-origin 消息，保留被压缩或回退遮蔽的用户可见原文。
- `events()`：完整事件账本，供恢复、Trajectory、审计与分叉读取。

三种投影共享同一个 Surface 状态转换实现。任何消费者都不得独立重写 replace 语义。

## Turn、工具与恢复不变量

- 每个 turn 恰好一个持久化终态。
- 每个 assistant 工具调用最终恰好一个工具结果；真实、拒绝、取消、未开始和结果未知都属于结果。
- 模型请求前、工具执行前与 step 边界使用 fail-closed 持久化检查点。
- 崩溃恢复只追加确定性的关闭事件，不修改物理尾部。未记录工具开始时生成 `not-started` 结果；已记录开始但没有结果时生成 `outcome-unknown` 结果。
- steering 继续由 shell 的 `expected_turn_id` 栅栏治理；Timeline 只记录被接受的输入与结构化终态。

## 压缩与缓存

压缩事务由 `compaction/start`、replacement message 与 `compaction/end` 表达。替换只遮蔽 Surface，原消息仍在 Timeline 与 transcript 中。工具结果 pre-prune 是单节点 content-only replacement，不能整表覆盖。

模型上下文保持“冻结头 + 活动尾”纪律：

- 压缩只推进头部覆盖边界；
- 动态 runtime context、工具目录与 reminder 只追加到活动尾；
- `prompt_cache_key` 由 workspace、Timeline lineage 与 model 构成，不含消息数和时间；
- 覆盖前缀指纹只哈希实际发送给 provider 的 wire 字段；
- cache warm/cold/unknown 只用于观测，不能触发历史改写。

## 持久化格式

Timeline 使用独立的 append-only `timeline.jsonl` 流，每个事件携带 schema 版本。加载时先校验版本、seq 连续性与事件结构，再构建 Surface；坏事件不能降级成“尽量恢复”的消息数组。

Grow 不为旧的可变 `chat_history.jsonl` 格式保留执行兼容层。新格式加载只认 Timeline；投影缓存损坏或缺失时从 Timeline 重建。分叉把裁剪和变换后的 Surface 物化为新 Timeline 的 Seed lineage，回退追加 replacement 事件并保留旧事实。

## 模块所有权

| 模块 | 所有内容 | 不得拥有 |
| --- | --- | --- |
| `chat-state::timeline` | 事件、seq、Surface fold、投影、校验 | sampler、shell 状态、文件 IO |
| `chat-state` actor | Timeline 的串行写入口、token/usage 投影、请求组装 | 可变历史副本、第二份事实日志 |
| shell session | turn/foreground/steering/Behavior、事件生产 | Surface 重写实现 |
| session storage | Timeline 追加、flush、前缀复制 | 会话语义与消息修复 |
| sampler | 传输、流转换、重试、取消 | 会话压缩与历史恢复 |

架构来源为 `/Users/lordcasser/workspace/projects/solaris/docs/agent-core` 的 T/P/G 模型、`deepseek-harness` 的 session/surface fold，以及 Grow 已验证的 sampler、steering、权限与压缩策略。实现冲突时以本文模块边界和不变量为 Grow 的代码约束。
