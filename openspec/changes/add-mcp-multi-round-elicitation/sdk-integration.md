# rmcp 3.5.1 集成核查

2026-10-11：选定官方 `rmcp = "=3.5.1"`，完成依赖/API 适配和最小 fixture。可以保留这次依赖升级，但 **M0 的交互接入门槛没有通过**：自动 helper 缺少必要的宿主接点，先解决上游扩展，再接 Shell/Pager。

## 版本与依赖

[crates.io 版本 API](https://crates.io/api/v1/crates/rmcp/3.5.1) 的发布记录为 2026-10-05、未 yanked、MSRV 1.88；[官方 tag](https://github.com/modelcontextprotocol/rust-sdk/releases/tag/rmcp-v3.5.1) 和实际下载的 crate 对应此版本。精确固定版本是为了使后续 SDK 升级重新经过接点验证，不依赖浮动 `main`。

| 项目 | SDK 要求 / Grow 实际结果 | 处理 |
| --- | --- | --- |
| Rust | SDK 1.88；仓库 1.93.1 | 使用 rustup 的 1.93.1 实际编译；系统 Homebrew rustc 1.98.1 不作为工具链验证依据 |
| rmcp | 3.5.1，唯一直接依赖在 `mcp/Cargo.toml` | 下游继续通过 `mcp::rmcp`，没有新增兼容 crate 或 SDK fork |
| reqwest | SDK `^0.13.2`；实际 0.13.4 | 沿用统一版本，不引入 0.12 分支 |
| http | SDK `^1`；实际 1.5.0 | Grow wrapper 与 SDK 使用同一 trait 类型 |
| sse-stream | SDK `^0.2.4`；实际 0.2.5 | Grow wrapper 与 SDK 使用同一 SSE/error 类型 |
| TLS | `reqwest-tls-no-provider` → `rustls-no-provider` | 保持共享 ring provider；lock/tree 无 `aws-lc-rs` |
| feature | 仍仅显式开启 client、no-provider、async-rw、HTTP reqwest transport | SDK 的 SSE feature 带入 base64/bytes；未启用 server、macros、auth、JWT、request-state |
| lockfile | 仅替换 rmcp 包及其依赖边 | base64/bytes/indexmap 已存在于工作区；没有顺带升级其他包 |

`cargo metadata --locked --no-deps` 核对直接依赖入口，`cargo tree --locked -p mcp -i <package>` 核对实际传递图和 feature，实际 `mcp`/`cli` 编译核对 MSRV 与 trait。OHOS 只核对了依赖/feature 约束，本轮没有运行交叉编译。一次全工作区 `metadata --offline` 因未缓存的无关 `alacritty_terminal` 失败，不把它记成完整 resolved metadata 验证。

## 必要适配和保留的 workaround

`StreamableHttpClient::get_stream` 的 session id 从 `Arc<str>` 改成 `Option<Arc<str>>`。Grow 薄 wrapper 和 mock 一起适配，并通过实际无 session HTTP discovery fixture。`ClientInfo` 改为当前 `ClientConfig`；`Transport`、`ClientHandler` 的 RPIT 签名、ACP 桥接及下游类型通过编译。

保留 `McpHttpClient`：3.5.1 对 graceful EOF 会退避，但对已建立 stream 的 body error 仍立即重连并重置计数，见 [SDK SSE 源码](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.5.1/crates/rmcp/src/transport/common/client_side_sse.rs#L441-L491)。原 `repro_sse_flood` 的异常 body/节流/健康服务三项全部通过，不能只因 SDK 版本变了就删除。

保留 `CancelMcpCallOnDrop`：SDK `RequestHandle` 没有 Drop 自动取消；其 timeout 会发通知，但 Grow 的旧路径使用 `PeerRequestOptions::no_options()` 和外层 deadline，所以没有两个 timeout owner。旧取消/超时/替代 service 身份测试全部通过。

生产入口尚未改成现代 discovery 或自动 MRTR，也未声明 Form/URL。现有 stdio/HTTP/ACP 握手和单轮调用保留。现代能力只在 fixture 中启用，避免 SDK 升级被误认为宿主交互已完成。

## 实际 SDK fixture

源码：[`rmcp_sdk_contract.rs`](../../../crates/codegen/mcp/tests/rmcp_sdk_contract.rs)。所有续接均调用官方 `RunningService::call_tool` / `call_tool_with_mrtr_max_rounds`；脚本 peer 仅用于测试，不是产品协议实现。

| 已验证机制 | 实际观察 |
| --- | --- |
| 两轮 Form、复用相同 key | wire id 各轮不同；新轮没有 state 时不回传旧 state；答案按当前 key/轮次替换 |
| 最终 typed result | structuredContent、image block、isError 保留；图片解码/附件预算由原产品路径负责，此测试不证明图片有效 |
| 并行同 key | `tokio::task_local!` 可以把原调用上下文传到自动 helper 内同步驱动的 handler；两个 callback 的 SDK id 相同，不能拿它作为 Grow logical call id |
| state-only | SDK 自己按 50/100/200 ms 退避；三轮上限只发三次调用，不复制该算法 |
| 现代 HTTP | 经 Grow `McpHttpClient` 跑 SDK discovery 和自动续接；SDK 产生每轮 `_meta`、Mcp-Method/Mcp-Name header，没有 session id |
| legacy fallback | SDK `ClientLifecycleMode::Auto` 在 discover 的 correlated -32601 后回退 2025-06-18 initialize；无 Grow 版本协商代码 |

没有在生产中引入 task-local owner。上表只证明候选接点可行，不能证明 Shell owner/gate、UI 队列或取消已接通。

## 上游扩展缺口

以下四项有可运行的 `sdk_limitation_*` 反例。测试通过表示精确复现 SDK 的默认行为，**不表示对应 M-25 场景通过**。其中 owner/token 的绑定可以由 Grow 调用 scope 薄适配完成；另外三项及后面的派发/等待策略需要 SDK 接点，不能把全部宿主职责都推给上游。

| 缺口 | 反例 / 影响 | 必需接点 |
| --- | --- | --- |
| 调用级 capability 被覆盖 | discover 建立的 connection capability 在 `Peer::send_request_with_option_and_subscription` 中覆盖 params `_meta`；headless 调用显式传空能力，wire 仍有 elicitation | 自动 helper 能接受并在每轮保留调用级 request options/meta；不能靠另一会话的连接配置声明 Form/URL |
| 无整轮预检 | `fulfill_input_requests` 为私有 `try_join_all`；同轮 a-url + z-roots 已进入 URL callback 才返回 roots 不支持 | 首个 handler 前提供整轮 `InputRequiredResult` 的异步准入回调，让 Grow 原 owner 预留数量/字节预算或拒绝整轮 |
| 嵌套 token 不关联 owner | `input_request_context` 新建 token，id 取 input key；原 owner 被取消并 drop helper 后，callback token 仍未取消 | Grow 用调用 scope 显式携带原 owner/token，由原 owner 取消 waiter、RAII 清理 pending；不拿 SDK key/token 冒充原身份。轮次身份仍依赖整轮接点，活跃 wire 的取消见下一项 |
| 自动调用无法取得活跃 handle | helper 内部 `peer.call_tool_once` 使用无选项请求；drop 自动调用不发原 wire cancelled | 每轮沿 SDK 提供 request options/活跃 handle 生命周期或 SDK 的 cancel-on-drop，使 Grow 可限定网络 deadline，并对原 peer/id 至多一次取消 |

另外，公开自动 helper 只接受 params 和 max_rounds。`prepare_input_required_retry`、整轮聚合、state-only sleep 都是私有函数；没有等待策略/手动检查唤醒或 continuation 派发前回调。外层 `timeout(call_tool(...))` 会同时计算人类等待，无法代替单轮网络 deadline；在 handler 内检查 gate 也覆盖不了 state-only continuation。这两项需要随上游接点一起验证，当前只有源码证据，还没有可接入的 Grow 路径。

固定证据：[自动 helper 与私有 round 处理](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.5.1/crates/rmcp/src/service/client.rs#L1966-L2215)、[Peer metadata / request options](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.5.1/crates/rmcp/src/service.rs#L772-L927)。额外核对官方 main `c1a8eea312613519b93f53b89a1dc74dc993f5b0`，上述 helper 签名和私有阶段仍相同；没有切换到未发布代码，也不以 main 代替已编译版本。

这里需要的是 SDK 暴露已有阶段的宿主接点，不是 Grow 再写 state 聚合/回传、协议协商或 MRTR loop。本轮未提交上游 issue/PR，避免把本地核查自动变成外部通信。

## 继续接入的条件

先在官方 SDK 上补齐或取得以上接点，用当前 fixture 验证调用级能力、整轮失败无副作用、逐轮取消/网络期限、state-only 等待和 continuation gate。然后才在 `mcp` 绑定 Grow logical call，并接既有 Shell owner/pending 和 Pager 交互。具体 Form/URL 服务、schema 支持表、预算、期限和 localhost 策略仍在 tasks 2.1，不能由这些 SDK fixture 推导已选定。

M-24 的本机依赖/API/SSE/取消检查已通过；M-25 已完成探针并发现阻塞，交互验收未通过。当前 change 保持 active。
