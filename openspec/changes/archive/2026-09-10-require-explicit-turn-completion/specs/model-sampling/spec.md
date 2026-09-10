## MODIFIED Requirements

### Requirement: Portable boundaries keep tool exchanges together

Portable prefix 与 live suffix 的切点 SHALL NOT 将同一完整工具往返拆成被删除的调用和孤立结果。生成请求时 SHALL 在现有消息/native span 边界内闭合连续工具结果；wire 转换、请求 token 估算及投影证据 SHALL 使用一致的范围。

#### Scenario: Unsigned response precedes tool execution
- **WHEN** 完整 Messages 响应的 unsigned thinking 使 native 撤下，assistant 持久化后工具执行并追加结果
- **THEN** 下一请求保留该调用和结果各一次，不带无效 thinking/signature，已执行工具不重放。

#### Scenario: Several results straddle the prefix
- **WHEN** 同一批工具的多个结果分处 prefix 两侧
- **THEN** 投影保留完整配对及图片，估算与实际投影一致，不吞并后续 native span 或新消息。

#### Scenario: Normal final answer
- **WHEN** 修复后的普通模型请求收到合法 provider 终止且没有业务工具调用
- **THEN** 仍须通过显式 Turn 完成协议；末尾标点不参与判定，合法响应结束不自动表示任务完成。

## ADDED Requirements

### Requirement: Ordinary turns require explicit completion intent

普通模型 Turn SHALL 使用宿主提供的 FinishTurn 工具明确声明 completed、waiting_for_user 或 waiting_for_background，并附非空理由。该声明 SHALL 是本响应唯一工具调用且伴随非空用户可见文本；宿主 SHALL 先持久化调用与结果，再允许正常结束。provider 的 end_turn、stop、response.completed 只结束一次响应，SHALL NOT 单独构成 Turn 完成授权。结构化输出、provider 拒绝、控制终止、取消和预算终止 SHALL 保留独立终止路径。

#### Scenario: Action preamble with a valid provider terminator
- **WHEN** Messages、Chat Completions 或 Responses 返回无工具行动预告及合法响应终止
- **THEN** 原始响应保留，追加协议纠正并经下一 Step 继续相同 Turn，不要求用户发送“继续”，不根据中文或英文冒号匹配。

#### Scenario: Explicit final answer or waiting
- **WHEN** 模型返回用户可见答案/问题和独立有效的 FinishTurn 声明，且没有新接纳输入
- **THEN** 记录声明结果并进入现有 Stop gate；Turn terminal 保留显式完成或等待种类，等待声明不自行完成或暂停 Goal。

#### Scenario: Mixed or invalid completion calls
- **WHEN** FinishTurn 与业务工具同批、重复出现、参数无效或没有可见回答
- **THEN** 完成声明被拒绝并写入匹配结果，FinishTurn 不进入业务工具派发；业务工具仍按正常权限路径执行，已执行的历史调用不重放。

#### Scenario: Repeated protocol violations
- **WHEN** 连续三次响应未提供有效完成声明，也未交付业务工具调用
- **THEN** 返回明确协议错误并停止采样，不记录正常完成；有业务调用后重新计算连续违约，模型切换本身不能重置计数。

#### Scenario: Control and pending input take precedence
- **WHEN** 恢复或完成期间出现取消、预算耗尽、控制切换或已接纳的新输入
- **THEN** 后续采样遵守原有 Step 边界和准入；取消/预算/终止控制不被完成协议重新打开，新输入不能沿用旧响应的完成声明。
