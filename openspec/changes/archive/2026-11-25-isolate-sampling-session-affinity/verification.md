# 会话分流验证

## 现场记录

2026-11-25，Grow 会话 A与 B在同一工作目录使用同一条 Messages 路由。两个会话的系统指令和第一条环境消息相同，用户分配的任务不同。取证只读访问 Grow 的 Timeline、落盘的提供方请求与响应以及 Magpie 日志。

| 对照 | 结果 |
| --- | --- |
| 独立会话与网关用量 | 按完成时间、输入和非零输出 token 数对齐，A 的 66 条、B 的 95 条可唯一匹配到同一个上游对话 `key-1`。 |
| 工具结果与上游历史 | 两个会话交错期间，桥接记录了 79 次 `reclaim:unmatched_results`，随后按 `no_live` 重放。其中一次从 592 条消息裁到 29 条，3,037,257 字节减到 102,725 字节。 |
| A 的首回合 | 50 分 31 秒内完成 295 次工具调用，包含 167 次读文件和 54 次加载 Skill。逐请求核对前一工具调用的 ID、参数和结果，Grow 的请求体持续保留成对的历史。 |
| A 的最后两个普通响应 | 请求体从 3,278,170 增至 3,278,379 字节，提供方报告的输入从 40,456 跳到 839,800 tokens。两次均为 HTTP 200，Messages 以 `end_turn` 正常结束，界面里的 `NOT_FOUND (code 404)` 出自助手正文。 |
| 随后的摘要 | 11:06:42 至 11:07:10 发生一次前台压缩。该操作三次提交同一份 2,584,506 字节的摘要请求，三次均收到流内 `api_error`。网关记录该账号无法选择 `gemini-4-neon-high`，该轮没有生成摘要或替换历史。 |

A 的本次增长发生在持续运行期间。两条普通响应的正文没有提供实体标识，无法据此判定其中的 404 与摘要失败来自同一个上游错误。

## 历史请求的离线重放

使用事发时安装的 AGY 插件，在本机对实际请求体执行 `requestSessionScope` 和 `conversationKey`。历史证据保留了请求体，没有保留完整 HTTP 请求头。旧版 Grow 的传输代码和当时的路由配置均未设置会话标识，按该输入计算出的键也与独立的网关日志吻合。

| 送入桥接的实际请求 | 原始键 | 传入各自的请求标识后的键 |
| --- | --- | --- |
| A 的开场、599 条消息和 601 条消息时的请求 | `key-1` | 三次均为 `key-2` |
| B 的开场 | `key-1` | `key-3` |
| 两个相同投影的独立 Sideband 查询 | 两次均为 `key-4` | `key-5` 和 `key-6` |
| A 的摘要 | `key-7` | `key-8` |

离线对照给两个主会话分别使用 Grow 格式的分支键，给三个 Sideband 使用各自的操作 UUID。请求正文保持逐字一致，未向真实提供方重发用户历史。新的上游键是夹具的计算值。

在与已安装 Magpie 0.1.4777 相同的 revision `5180600b` 上，启动本机 HTTP 网关处理器，使用该项目的测试插件和本机模拟提供方。对上述七个请求各发两次，仅将目的模型改为夹具路由。14 次均返回 HTTP 200。测试插件实际接收的新请求中，`X-Magpie-Session` 逐字等于入口的 `X-Session-Id`。每对转发请求的 JSON 完全相同，七组的 system、messages、metadata 均与存档一致。从插件实际收到的头和正文重算，结果与上表一致。

插件的 `replayHistoryProblem` 接受 A 的全部 295 个工具结果前缀。将真实调用 ID 分别注册为待回项后，`matchToolResults` 在相同分支作用域匹配 295/295，在另一主会话和摘要作用域合计拒绝 590/590。

## Grow 回归

使用仓库锁定的 Rust 1.93.1，在独立临时 target 中构建。所有 HTTP 回归连接本机测试服务。

| 测试入口 | 对应场景 | 结果 |
| --- | --- | --- |
| `cargo test --locked -p sampler --lib --test test_actor` | Sampler 库与 Actor 测试。新增测试从 HTTP 接收端检查 Chat Completions、Responses、Messages 的默认分支键、显式同名头及无标识请求，逐项断言头值与个数。 | 296 项库测试、30 项集成测试通过 |
| `cargo test --locked -p shell --lib sampling_session_affinity` | 暂停 A 的首个请求，运行同开场的 B、两次相同的真实旁路提问，再继续 A。验证 A 跨步稳定，旁路遇到 529 后重试保留身份，四个操作各自分流。 | 1 项通过，核对 6 个 HTTP 请求 |
| Shell 库测试的 `recap_display_only_tests`、`actor::sideband::`、`helpers::session_compact::`、`portable_replay_compaction_tests` | 摘要、旁路、近期工具证据和可重放历史的已有行为。 | 76 项通过 |
| `cargo test --locked -p chat-state --lib prompt_cache_key_tracks_timeline_model_and_rewind_lineage` | 同一分支追加消息稳定，改变模型、rewind 和 fork 各自换键。 | 1 项通过 |

`cargo fmt --all -- --check` 和 `git diff --check` 通过。

归档前 `openspec validate --all --strict --no-interactive` 通过 19/19。归档后同一命令通过 18/18，`openspec validate --archived --no-interactive` 通过 990/990。
