## MODIFIED Requirements

### Requirement: Named provider stream errors retain their error facts

Chat Completions, Responses, and Messages SHALL recognize a complete `event:error` SSE frame with nonempty `code` and `message` as a provider error, even when its JSON body lacks a `type` or nested `error` field. Grow SHALL retain the provider code, message, and supplied request ID for diagnosis. Error classification SHALL distinguish confirmed content rejection, throttling, and overload, while unknown codes and malformed ordinary events SHALL NOT gain automatic retry eligibility. Existing attempt settlement, output retraction, admission, tool execution, and shared retry budgets SHALL remain authoritative.

SSE 错误资格 SHALL 按正文普通事件身份判定，且 SHALL 只在顶层 `error` 成员存在时做资格校验：无 `error` 成员的帧（普通或 unknown extension payload，含非字符串未知成员）SHALL 交回既有 typed decoder 维持其原裁决，不被资格 gate 代替。帧携带普通事件身份（非 `error` 的非空顶层 `type`、Chat `choices` 成员或 `object:"chat.completion.chunk"`）且同时存在顶层 `error` 成员（含显式 `null`，按成员存在性判定）时，无论普通 typed 部分是否完整、也无论 `event:` 名是否为 `error`，该帧 SHALL 归为本地 `Serialization` 协议错误，SHALL NOT 作为 Api 服务错误取得重试资格或完成事件。`error` 成员存在时资格 probe SHALL fail-closed：已声明成员（`type`/`object`/`choices`/`error`）重复键或形状非法时 SHALL 同样返回 `Serialization`，SHALL NOT 落回会忽略未知顶层成员的既有错误分类；合法 bare legacy、完整 named error 与合法 typed error SHALL 保持既有分类、状态与 request ID 保留；未知扩展帧附加 error SHALL 归为 `Serialization` 而非取得自动重试资格。

#### Scenario: 普通事件身份与顶层 error 并存

- **WHEN** 任一 reader 的普通 SSE 帧（如 `type:"message_start"`、含 `choices` 的 chunk 或 `object:"chat.completion.chunk"`）同时携带 HTTP 风格顶层 `error`，即使普通 typed 部分完整或 `event:` 名为 `error`
- **THEN** 最终错误为不可重试的 `Serialization` 协议错误，不按 error 包分类为 Api/5xx，也不作为完成事件结算。

#### Scenario: doom-loop check 命名/typed 帧与顶层 error 并存

- **WHEN** Responses reader 收到 SSE `event:` 名或正文 `type` 标记为 `response.doom_loop_check` 的帧，但正文为合法 JSON 且同时携带顶层 `error` 成员（普通身份 + error 混合帧）
- **THEN** 该帧 SHALL 先经错误资格判定归为不可重试的 `Serialization`，SHALL NOT 被 check 吞掉路径吸收，也 SHALL NOT 先由 doom-loop collector 记录 signals；无 `error` 的合法 check 帧与 check 名下未构成完整 JSON 对象的正文（非法 JSON、标量、null 或数组）SHALL 保持既有吞掉/忽略语义，且不阻塞后续合法 terminal 事件完成。

#### Scenario: 合法错误包保持原分类

- **WHEN** SSE 帧为 bare legacy（`{"code":..,"error":".."}`，无普通身份）、完整 named error（非空 `code`/`message`）或合法 typed error（`type:"error"` + `error` 对象）
- **THEN** 沿既有共享分类产出原 Api/status/message/request ID；未知 code 保持不重试与 veto。

#### Scenario: 已有初始工具候选后混合帧

- **WHEN** 流先建立合法工具候选，随后出现普通身份 + 顶层 error 混合帧
- **THEN** 整次尝试按 `Serialization` 拒收，不产出 Completed 工具候选，不执行工具。

#### Scenario: Content rejection after provisional tool output

- **WHEN** a provider emits a partial tool candidate followed by `event:error` with `InvalidParameter`, a content rejection message, and a request ID
- **THEN** Grow reports the provider facts, settles usage as known or unknown according to actual evidence, rejects the entire candidate, executes no tool, and does not retry that rejection.

#### Scenario: Known transient stream error

- **WHEN** an `event:error` frame carries a confirmed throttling or service overload code
- **THEN** Grow uses the existing 429 or overload classification, and any retry remains bounded by attempt safety and the shared budget.

#### Scenario: Missing fields or unknown code

- **WHEN** a non-error SSE frame lacks a required event type, a named error lacks required fields, or a named error has an unknown provider code
- **THEN** missing-field frames remain protocol failures and unknown provider errors preserve their code/message without being assumed transient.
