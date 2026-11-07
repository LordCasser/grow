## ADDED Requirements

### Requirement: Responses terminal usage completion preserves original field conflicts

Responses terminal 事件（`response.completed` / `response.incomplete` / `response.failed`）缺 `input_tokens_details` / `output_tokens_details` 或其中 `cached_tokens` / `reasoning_tokens` 时，sampler SHALL 只在 typed-decode 副本中补齐这些明细默认值，且补全副本除插入的明细外逐字节保留原始 JSON 文本。原始已知字段（包括 `response.id`、function `name`/`call_id`、`status`、`output`、顶层 `type`/`sequence_number` 及 usage 已知字段）的类型、唯一性与身份冲突 SHALL 保持 SDK 原有拒收：同一原始字段错误在明细完整与缺失两种输入下 SHALL 得到一致拒收，不得因补全而变为可接纳的 Completed 候选。未知扩展键（含重复）的既有 SDK 容许习惯 SHALL 保持不变。补全 SHALL NOT 修改 raw evidence、usage 字段可用性（未报告与明确 0 的区分）、候选身份/终态/args 校验或既有恢复与结算归属，也 SHALL NOT 扩展到 summary/status/timestamps 等其它缺失字段。

#### Scenario: 合法缺明细的 terminal 仍完成

- **WHEN** 一条无其它冲突的 terminal 事件省略 usage 明细，后续无更多输入
- **THEN** decode 副本补零后正常解码完成，已知总量照常结算，未报告的 cache 读取/写入保持未知而非已知 0，raw evidence 保留原始字节。

#### Scenario: 重复已知字段不因缺明细被放行

- **WHEN** 同一 terminal 事件携带重复 `response.id`、function `name` 或 `call_id`、重复 usage 已知字段或 null-first `id`，且同时省略 usage 明细
- **THEN** 补全副本保留全部重复字节并被既有 typed 校验拒收；真实 HTTP/SSE → client → L2 链路不产出 Completed 工具候选，与明细完整时的拒收一致。

#### Scenario: 初始候选之后的 terminal 冲突仍拒收

- **WHEN** 流先给出合法初始 function_call/text part 事件（可使用既有初始例外），terminal 事件再携带重复已知字段且缺 usage 明细
- **THEN** 整个候选仍按原始冲突拒收，既有初始例外不放大为对 terminal 冲突的接纳。

#### Scenario: 补全资格限定对象形状

- **WHEN** usage 或已存在的明细容器不是 JSON 对象——包括 serde 派生 struct 直接可解码的序列形式（如 `[0]`）——或对象容器内的计数字段为非法类型
- **THEN** usage 补全只认对象形状并在此不触发：SDK 可直接解码的完整序列形式保持原状不改写；序列形式与另一明细缺失并存时保持该缺失的原有拒收，不因补全另一容器而放行；`null` 容器与非法计数保持既有解码失败。

#### Scenario: 未知扩展重复键保持既有容许

- **WHEN** 事件携带 SDK 未知的扩展键且重复出现，usage 明细缺失
- **THEN** 补全后的事件仍按既有 unknown-field 容许规则解码，其接纳结果不因本补全边界改变。
