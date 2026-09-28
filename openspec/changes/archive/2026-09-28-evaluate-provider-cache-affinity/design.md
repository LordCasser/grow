## Context

主请求的 `prompt_cache_key` 由 timeline、rewind 分支、backend、base URL、wire model 和 native epoch 计算；Responses 发出该 key，Chat 不传，Messages 在 system 和消息边界放置 `cache_control`。recap 使用 session ID 作为独立 key，且不带主请求 tools。它与主请求有相同会话来源，但这不能证明远端缓存前缀相同。

三个前置 change 已归档：图片投影选择在纯追加时稳定；cache read/write 分别保留字段可用性；离线工具能对照可见请求、响应和已结算 usage。它们修复本地证据边界，不提供真实 provider 命中数据。

## Goal / boundary

本 change 用官方文档和当前代码做理论论证与 review，回答现有 key 生命周期、breakpoint、retention 是否有足够依据统一调整。用户将真实请求和跨 provider 实测比较移出本次验收范围，因此不运行 provider 请求，也不推算命中率、延迟、费用收益。

不合入生产缓存参数或新的策略抽象，不改变 native 续传安全边界。后续需要真实 route 验证的点留在 backlog；理论上可行不等于当前模型、网关或部署已验证。

## Review method

1. 从已归档规范和实现定位主请求 key、Responses/Chat/Messages wire 参数、recap Sideband 形状，逐一核对测试实际断言。测试只证明本地 wire 与状态，不冒充远端缓存结果。
2. 查当日 [OpenAI](https://developers.openai.com/api/docs/guides/prompt-caching)、[Claude](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)、[DeepSeek](https://api-docs.deepseek.com/guides/kv_cache/) 官方文档及适用的 API reference。`route-matrix.md` 分开写 provider 契约和本机 route 身份；无法确认模型别名、上游、部署或字段透传时保留未知。
3. 按下面的机制问题审阅 `results.md`。结论只允许“保留现状”“另开行为 change”“证据不足”，并逐项给出来源、推理前提和无法由文档推出的事实。
4. 用合成 fixture/mock 核对未来取证所需的身份、可用性和预算停止；输出是测试工具证据，不是 provider 读写量。修正 recap 里把 wire 形状写成命中保证的注释/测试名称。

| 机制问题 | 静态 review 的核对点 |
| --- | --- |
| 纯追加与图片回收 | 本地 Surface 选择是否稳定；provider 可见前缀仍受编码、图片和模型参数影响。 |
| native reset / rewind / fork | key 和 epoch 如何变化；native 合法性与缓存亲和是不同约束。 |
| tools、system、effort、输出格式 | 可见前缀与 settings 的变化；不同 provider/model 的断点规则不可合并。 |
| Messages 长尾追加 | `cache_control` 位置、上轮边界和官方 lookback/TTL；不推断实际读写量。 |
| recap / Sideband | 独立 key、tool-free wire 和冻结来源；不能将来源相同当作主请求缓存命中。 |
| TTL / 并发 | 官方生命周期和隔离规则；无 route、发送间隔与服务端证据时不判断收益。 |

## Decision boundary

OpenAI 的 key 含义随模型代际变化，Claude 使用 breakpoint，DeepSeek 文档描述自动缓存；协议兼容网关不能代替官方 route 身份。因此当前没有足够依据引入跨 provider `CachePolicy`、共享 key 或统一 retention 默认值。允许纠正不准确的注释，其他行为候选要先有具体 route 和可验证收益，再另建最小 change。

归档只表示本次理论论证和 review 完成。`results.md` 的未执行场景和未知项继续保持未执行；mock 不补这些格子。
