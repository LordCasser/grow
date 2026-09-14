## MODIFIED Requirements

### Requirement: Session usage is a durable lifetime projection

Session Timeline SHALL 持久记录每个主模型 attempt 的已知/未知 Usage、每个子 Agent 的最终 Usage 结算，以及 session-level incomplete 事实。冷恢复 SHALL 按结算身份去重重建 lifetime aggregate、provider/model 分项和 Agent 归属；不得把已有消费重置为零或重复计费。已知用量事件 SHALL 在 actor 发布及恢复事件持久化前完成载荷校验；格式损坏或同身份冲突 SHALL 使恢复失败并保留原始记录。

#### Scenario: Restore settled main attempts
- **WHEN** Timeline 含多个已持久化主模型 attempt 结算并创建新的 actor incarnation
- **THEN** 新 actor 在接受后续调用前恢复这些 attempt 的累计 token、model、cost、duration 与 incomplete 状态，每个 attempt 至多计入一次。

#### Scenario: Restore settled child usage
- **WHEN** 父 session 已持久结算一个子 Agent 的多模型 Usage 后冷恢复
- **THEN** lifetime 总计、provider/model 分项与该 `subagent_id` 的 Agent 分项均恢复；相同结算重放不重复累计，冲突结算失败关闭而非覆盖。

#### Scenario: Restore incomplete accounting
- **WHEN** session-level incomplete 事实已持久化
- **THEN** 后续 resume 仍将 lifetime Usage 表示为已知下界并隐藏不可信 cost，不因进程重启恢复为精确账本。

#### Scenario: Exact duplicates and conflicting attempt payloads
- **WHEN** 同一主模型 attempt 或子 Agent 身份有多份用量结算
- **THEN** 完全相同载荷只计一次；任一载荷冲突使恢复失败，不选择第一份、不覆盖，并采用与实时结算相同的冲突规则。

#### Scenario: Malformed known usage facts fail before publication
- **WHEN** 已知 attempt/child 结算无法按既有 typed schema 解码，或 incomplete/resume 标记包含不属于其格式的 data
- **THEN** 恢复返回带事件位置的错误，不发布可用 actor、不写入恢复事件，原持久化记录保持不变。

#### Scenario: Unrelated diagnostic observations remain extensible
- **WHEN** Timeline 包含不属于已知用量 scope/name 的合法 Observation
- **THEN** 用量恢复忽略该诊断事实，不将其当成损坏的结算；其他 Timeline 校验仍执行。
