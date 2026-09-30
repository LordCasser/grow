## Why

用户在生成中补充输入是正常交互；被打断的请求可能没有返回最终 usage。当前普通状态栏因此将整个会话的缓存率持续显示为 `N/A`，即使已有大量可信缓存数据，用户也无法判断缓存是否生效。会话 `01a0eaf7-3acf-7872-8084-f4432d1c57a2` 已记录输入的命中率为 97.06%，两次 `steered` 请求的未知用量却使状态栏一直失去该信息。

现行 `client-surfaces` 契约明确要求这种 N/A，故本项修改展示契约。同时修复源码审计发现的有限结算窗口：Chat 已解析并确认完整 usage、仍等待结束帧时，取消可能丢掉解析器内的 usage。该窗口尚未通过运行测试复现，不能断言它导致了上述两次未知结算；实施须先补确定性回归。证据与边界见 [analysis.md](analysis.md)。

## What Changes

- 普通状态栏独立表达总量是否完整、缓存率是否可计算。只要已有有效样本，就显示样本缓存命中率；总量或 read 覆盖不完整时，在比例前明确显示 `measured cache`。示例：`≥36.4M tokens · measured cache 97.06%`。
- 保留 `≥`、未知消费事实、费用隐藏及精确预算约束；后续正常请求继续更新已测量缓存率，重复快照、子任务结算和冷恢复复用现有账本。
- Chat 在现有规则确认完整 usage 后，将其保留到该 attempt 的取消/期限结束结算；尚未确认的 usage 继续走未知结算，不增加为等待用量而延长生成的行为。
- 验证取消与终态竞争、重复打断、部分缓存覆盖、零命中、恢复和窄屏展示；同步开发者说明与 `/usage` 用户说明。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `client-surfaces`：普通状态栏在总量或缓存覆盖不完整时仍展示明确限定为 measured 的可计算缓存率。
- `model-sampling`：取消及逻辑期限结束必须保留该 attempt 已确认的完整 usage，并沿既有幂等账本结算。

## Impact

实现边界为 `sampler/src/stream/chat_completions.rs`、`sampler/src/actor/request_task.rs` 与 Pager 的 usage 展示；路径相对于 `crates/codegen/`。Shell 结算、ChatState 恢复及 ACP 累计快照作为集成验证边界。复用现有 `TokenUsage`、attempt identity、`AttemptUsageSink`、`UsageTotals` 和 `PromptUsage`；不新增持久化字段、公共事件、依赖或第二套账本。

不改变 provider 缓存策略、请求内容、响应接纳、重试资格和取消响应性；不估算或补零未知消费，不从 Messages 中间 delta 推导精确总量，不重算或改写旧会话事实。当前请求交付分析和立项，代码实施与归档保留为未完成任务。
