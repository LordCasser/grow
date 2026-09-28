# 理论结论

验收口径是官方文档、实现与测试的静态 review，加上离线 mock。未发送 provider 请求；以下没有命中率、延迟、费用或跨 provider 实测结论。

## 当前机制

| 对象 | 本地证据 | 能推出什么 | 不能推出什么 |
| --- | --- | --- | --- |
| 主请求 key | `chat-state/src/actor/request_builder.rs::prompt_cache_key` 将 timeline、最近 rewind、backend、base URL、model、epoch 组合哈希；纯追加保持 key，rewind/fork/route/native epoch 可改变 key。`sampling-types/src/conversation.rs` 只在 Responses wire 发送该字段。 | Grow 不会把不同分支或 route 的请求主动归到同一个 key。 | 同 key 不保证落在相同机器、存在缓存项或有相同可缓存前缀；不同 key 也不能单独量化损失。 |
| 图片与请求前缀 | 已归档图片投影 change 在未触发高水位时复用 Surface 的回收选择；离线工具能比较可见请求 JSON。 | 本地图片 placeholder 不会因纯追加无故反复变化。 | 原始 JSON 相同不等于 provider 渲染/token 前缀相同，也不证明图片前缀命中。 |
| Messages breakpoint | `sampling-types/src/conversation.rs::apply_cache_breakpoints` 在 system、最新可标记消息和较早 user 边界放置 `ephemeral` 标记；测试验证 wire 位置。 | 该函数最多新增三个缓存边界；原生片段可能携带已有标记，须按实际 wire 核对总数。 | 不能从标记数量推出远端读写量，长尾是否命中仍取决于模型、块序列、TTL 和服务端状态。 |
| recap Sideband | `shell/src/session/actor/recap.rs` 使用 session ID 独立 key；wire 测试确认不发送主请求 tools。 | recap 与主请求的 key/工具前缀不同；原测试只验证本地请求形状。 | “复用主请求缓存”不是该测试能证明的结论。保留 reasoning 也不能补足这个证据。 |

## 官方契约与适用边界

- [OpenAI Prompt Caching](https://developers.openai.com/api/docs/guides/prompt-caching)：GPT-5.6 及以后由服务端处理缓存路由，`prompt_cache_key` 可用于分开缓存计量；更早模型可用稳定 key 改善相关请求路由。新代际支持显式 breakpoint 和 `prompt_cache_options.ttl`，旧代际的 breakpoint/retention 规则不同。前缀还包含工具、指令、历史等渲染内容；key 或可见消息相同不保证命中。本地没有已确认的 OpenAI 直连模型/部署，因此这些是模型条件，不是当前 route 的实测性质。
- [Claude Prompt Caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)：缓存按 `tools`、`system`、`messages` 顺序直到 `cache_control` 边界；默认 5 分钟，可按支持情况选 1 小时，lookback 有块数限制。Grow 的 Messages breakpoint 与该接口形式相符，但本地只有上游身份与透传未知的代理，不能套用直连 Claude 结果。
- [DeepSeek Context Caching](https://api-docs.deepseek.com/guides/kv_cache/) 与 [Responses API reference](https://api-docs.deepseek.com/api/create-response/)：缓存由服务端自动处理并按已持久化的前缀单元 best-effort 复用；Responses usage 提供 `input_tokens_details.cached_tokens`。官方 Responses reference 当前列出 `deepseek-flash`、`deepseek-v4-pro`，本地 `deepseek/deepseek-v4-flash` 的别名/模型对应未确认；不能把 Chat 文档里的 hit/miss 字段直接当作这条 Responses route 的实测返回。

## 逐项判断

| 候选 | 本次决定 | 理由与后续条件 |
| --- | --- | --- |
| 统一延长 key 生命周期或跨 epoch/branch/Sideband 共享 key | 保留现状；统一策略证据不足 | key 同时承载分支和 route 隔离。OpenAI 不同模型代际对 key 的用途不同，Claude 不使用该请求字段，DeepSeek 文档描述自动缓存。放宽 key 也不能修复工具或内容前缀变化。若有具体 route 的收益假设，另建行为 change 并核对隔离边界。 |
| 新增或移动显式 breakpoint | 保留 Messages 当前标记；其他 route 证据不足 | Claude 的机制支持显式边界，Grow 已有 system/消息边界。OpenAI 新代际支持显式模式，旧代际不支持；当前无确认直连模型，也无匹配前缀的使用数据。DeepSeek 无同等请求控制。任何调整需具体模型与实际输入形状。 |
| 设置统一 retention / TTL | 保留 provider 默认值；统一策略证据不足 | OpenAI 和 Claude 的寿命控制及写入成本随模型/平台变化；DeepSeek 的清理时间不是客户端 TTL。没有请求间隔和服务端用量，不能判断较长寿命是否值得付费。 |
| 宣称 recap 复用主请求缓存 | 撤销该声明 | recap 的独立 key 和 tool-free wire 已由代码与测试确认；它们不足以证明远端复用。已改注释与测试名，实际收益留作按 route 验证。 |

## 原实验矩阵的状态

| 场景 | 本次静态结论 | 实测状态 |
| --- | --- | --- |
| 静态前缀连续追加、图片追加 | Grow 的图片选择和 key 可保持稳定；provider 前缀/命中另有边界。 | 未执行。 |
| native reset、rewind、fork | key 变化与 native 合法性可由本地代码分别说明。 | 未执行。 |
| tools/schema、effort、输出格式 | 改动可影响渲染前缀或可比较性，取决于模型/协议。 | 未执行。 |
| Messages 长尾与 breakpoint | 当前有显式标记，实际 lookback 和写入量未观察。 | 未执行。 |
| recap/Sideband | 独立 key、tool-free 形状，不能承诺主缓存命中。 | 未执行。 |
| TTL、发送间隔、并发 | 官方规则提供假设，缺 route 和时序用量不能判断收益。 | 未执行。 |

mock 仅验证合成证据格式及预算停止，不填入上表的实测格。无收益、反向收益、样本数和精确费用均没有真实观测值。
