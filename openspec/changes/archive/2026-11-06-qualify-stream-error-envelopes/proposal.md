# Qualify stream error envelopes by ordinary-event identity

## Why

backlog「流错误预解析的本地契约与资格」项（`openspec/backlog.md`）：共享 `try_parse_stream_error` 在三 reader typed 解码前运行，任何携带 HTTP 风格顶层 `error` 成员的普通事件（非 `error` 的非空顶层 `type`、Chat `choices` 或 `object:"chat.completion.chunk"`）都被 `parse_structured_error` 当作服务错误，`server_error` 形状可取得有限自动重试，绕过 Grow「malformed ordinary 事件是本地协议失败」的契约。这不是 OpenAI 官方协议违例（官方 SDK 同样先识别顶层 error），但 Grow 自身契约要求普通事件身份优先：普通身份与顶层 error 并存时无论普通 typed 部分是否完整，都必须是 `Serialization` 协议错误，不得作为 Api 重试或完成；`event:error` 事件名不能覆盖正文的普通事件身份。政策已由 main 协调 design 定稿（见 `design.md`）。

## What Changes

- `crates/codegen/sampling-types/src/error.rs` 的 `try_parse_stream_error` 增加最小 SSE 资格门：用 serde 窄 probe（顶层 `type`/`object`/`choices`/`error` 四成员，derive 自带重复键拒收）识别「普通事件身份 + 顶层 error 并存」，命中即返回 `SamplingError::Serialization`。probe 解析失败（含重复 `type`/`error` 键等已知边界）不做任何放行，落回既有分类路径。
- 既有分类保持不变：无普通身份的 bare legacy（`{"code":..,"error":".."}`）、完整 named error、typed error（`type=="error"` + `error` 对象）继续沿 `parse_structured_error`/named 路径产出原 Api/status/message/request_id；未知 code 仍 veto；未知扩展帧附加 error 不因 probe 取得重试资格。
- `parse_structured_error` 的 HTTP 非 2xx 契约不动；sampler 三 reader（chat 1508 / responses 1868 / messages 2164）在 typed decode 前已统一调用该 helper，生产代码无需改动，仅补真实 HTTP/SSE → `conversation_collect` L2 回归。
- 不用 `Value` 重编码或删 error 后接纳，不复制 SDK known-event 名单，不整体收紧 HTTP 非 2xx parser，不改候选、usage、retry 总预算，不碰 Anthropic code 分类（另项）。

## Impact

- 代码：`crates/codegen/sampling-types/src/error.rs`（helper + 同文件 tests）、`crates/codegen/sampler/src/client.rs`（仅新增 L2 回归 tests）。
- 契约：`openspec/specs/model-sampling/spec.md` Named provider stream errors requirement 的 delta，见 `specs/model-sampling/spec.md`。
- 行为影响：普通 SSE 事件附加 HTTP 风格顶层 error 从「按 error 包分类的服务错误（可能取得 5xx 有限重试）」变为不可重试的本地 `Serialization` 协议错误；合法 bare legacy / named / typed 错误包、HTTP 错误 parser 行为不变。
- 非目标：Anthropic 标准 `api_error`/`timeout_error` code 分类（backlog 另项）；不新增政策框架。
