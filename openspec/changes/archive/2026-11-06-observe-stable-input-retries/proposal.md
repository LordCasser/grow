# Change: observe-stable-input-retries

## Why

同一 session/promptId 的不可变请求重试目前被各入口当作新失败或伪装完成处理：
FIFO 接纳把 Allowed 重试当成 `RemovedFromQueue` 假完成，Consumed/Handled/Submitted
重试变成错误字符串；运行态派生的 Prompt vs Followup intent 差异被误判为身份冲突。
客户端丢 ACK 后无法观察原 canonical 输入身份/状态，且现有路径依赖遍历读取所有
历史 Submitted blob 扫描 prompt 归属。

## What Changes

- Shell 接纳入口先用稳定 `input_id_for_prompt(session, prompt)` 查询
  `ChatStateHandle::input_state`（Timeline canonical fold，提供方已实现），命中时
  只读该 input 的 payload_ref 核对完整不可变 payload/requested_route（沿用
  `comparable_input_payload`，仅排除服务端冻结 skill_invocations）；不再遍历
  历史 Submitted blob 扫描。
- 服务端在 FIFO idle/running 边界派生 Prompt/Followup intent 不构成重试冲突；
  显式 Steer 与 requested_route/正文/metadata 变化仍冲突。
- 重复请求是观察，不重复 Hook、接纳、入队或执行：所有 canonical 状态
  （submitted/allowed/blocked/consumed/handled/dismissed）的重试统一返回
  新的内部 `PromptCompletionKind::AdmissionReplay { input_id, state }`。
- ACP `session/prompt` 为本请求设置 `persist_ack`，优先消费应答：重复分支在
  normal completion/usage/roster/TurnCompleted 之前短路，返回 EndTurn 观察应答，
  meta 只含 sessionId、promptId 与 `grow/inputAdmission`（inputId + 按
  input/admission_status 形状的 status）。不发布 Working/Idle、不附
  totalTokens/usage/structured output、不伪造新 Turn 结束。重复分支先回应答
  再回原 persist 确认；未取得实际提交确认的接纳错误不发布假 Idle。
- queue 编辑保存路径复用 `admit_human_input`，显式处理 replay：不重插、不把
  blocked 重试当 fresh 失败。

不包含（延期）：F1 挂起 steer 恢复、F2 FIFO edit 排序。

## Capabilities

### Modified

- input-admission: 新增稳定重试观察的 delta（AdmissionReplay 应答、canonical
  只读命中核对、intent 派生不冲突、无假 RemovedFromQueue/Idle/TurnCompleted）。
