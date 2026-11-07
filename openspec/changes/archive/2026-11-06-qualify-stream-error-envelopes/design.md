# Design: qualify-stream-error-envelopes

## 决策（沿用 main 协调定稿政策）

- 资格门只看正文的普通事件身份与顶层 `error` 并存，与 `event:` 名无关（`event:error` 不能覆盖普通正文身份）。
- 普通身份定义（封闭，不复制 SDK known-event 名单）：顶层 `type` 为非空且不等于 `error`；或存在顶层 `choices` 成员；或 `object == "chat.completion.chunk"`。
- 命中即返回 `SamplingError::Serialization`（不可重试），即使普通 typed 部分完整也不得作为 Api 或完成事件。
- 实现载体：`try_parse_stream_error` 内的 serde 窄 probe，声明上述四个成员；serde derive 对重复的已声明键（`type`/`error` 等）天然报错。`choices`/`error` 用 presence wrapper（`MemberPresence`：serde 字段默认=Absent，任何反序列化值含显式 `null`=Present，经 `IgnoredAny` 忽略内容），不经 `Value` 重编码、不删 error 后接纳，不手写 JSON scanner，不新增依赖。
- **只在 `error` 成员存在时做资格校验**：两阶段 probe——第一阶段 `ErrorPresenceProbe` 仅声明 `error` 成员存在性（`MemberPresence`），无 `error` 成员的帧立即交回既有 typed decoder（无错误的 unknown extension payload、重复未知 `object` 等维持 decoder 原裁决，不被 gate 代替）；`error` 存在才进入第二阶段 full probe（`type`/`object`/`choices`），此时 malformed/重复已声明成员 fail-closed 返回 `Serialization`，不落回 legacy server-error 分类。两阶段均有实际边界依据（mixed-error 资格 vs decoder 自主权），不建 framework。
- **fail-closed（仅限有 error 的帧）**：probe 解码失败（非法 JSON、重复 `type`/`error` 键、已声明成员形状错误）时共享 gate 直接返回 `Serialization`，不落回 legacy 分类——legacy `ErrorResponse` 只声明 `error`、忽略未知顶层成员，`{"type":"a","type":"b","error":{server_error}}` 这类混合残缺包若落回会被当作可重试 server `Api`。普通帧（无 `error` 成员）不受 gate 裁决；合法 bare/named/typed 错误包含 `error` 成员且形状合法，full probe 通过，不受影响。HTTP `parse_structured_error` 契约不动。
- 共享helper拥有三reader的错误资格判定；Responses reader必须在吞掉check前调用。provider可把普通+error混合帧标记为`response.doom_loop_check`，不能让命名绕过资格。只有`is_check_event(name,data)`且`serde_json::from_str::<HashMap<String, serde::de::IgnoredAny>>(data)`确认正文不构成完整JSON对象时保留既有check忽略语义（非法JSON、标量、null和数组）；其他帧先调用`try_parse_stream_error`，命中即拒收且collector不记录signals，再走原check收集/吞帧或typed decode。合法check及unknown extension无error的既有行为保持。
- 保持不变：`parse_structured_error` 的 HTTP 非 2xx 契约；bare legacy / 完整 named / 合法 typed error 分类与 request ID 保留；未知 code veto；候选、usage 结算与 retry 总预算；Anthropic code 分类（backlog 另项）。

## 备选与取舍

- 收紧 `parse_structured_error` 本体：会波及 HTTP 非 2xx 契约，越界，否决。
- 在 reader 各自判断：三处重复同一语义且可能漂移，共享 helper 是共同所有者，否决。

## 验证

Tier 3：error.rs 同文件单元测试（成对覆盖普通+error、普通无 error、`event:error` 名覆盖、重复键不放行、bare/named/typed 保持）+ client.rs 三 reader 真实 axum SSE 服务器 → `conversation_collect` L2（含已有初始工具候选后混合帧整次拒收、无 Completed 候选）。
