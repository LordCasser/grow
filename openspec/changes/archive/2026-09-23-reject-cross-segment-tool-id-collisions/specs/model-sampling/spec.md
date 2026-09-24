## ADDED Requirements

### Requirement: Cross-segment tool IDs identify one exchange

在一个 provider 请求中，工具关联 ID SHALL 只标识一个调用。原生 span 的工具调用 ID 若被 span 外另一个中性 Assistant 调用复用，请求投影 SHALL 按完整中性历史重新投影，并拒绝该 ID 的歧义工具协议；此回退 SHALL NOT 改写 Timeline 或执行工具。原生调用的持久化镜像位于同一 span 内，及其后续中性结果，SHALL 仍可保留原生 continuation。

#### Scenario: Neutral and native exchanges reuse an ID
- **WHEN** a neutral assistant call outside a native span reuses the ID of a distinct native tool use inside that span
- **THEN** request projection discards native continuation for that request and omits the ambiguous call/result protocol through the full portable projection while preserving other conversation facts; no provider request contains both owners of the same ID.

#### Scenario: Native use has a later neutral result
- **WHEN** a native tool use's durable assistant mirror is inside its span and a matching neutral tool result follows it
- **THEN** the native span and result retain their shared ID and are each encoded once without falling back to portable projection.
