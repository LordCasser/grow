## ADDED Requirements

### Requirement: Resume restores ordered communication history

Resume SHALL 恢复普通会话的原历史展示，不把历史恢复当作新对话或 Replay 播放页面。Timeline-only 通信回执 SHALL 与既有正文按可验证因果锚点合并，保留稳定 receipt identity、原正文/通信样式和来源时间；加载后 SHALL NOT 把全部历史回执重新追加到最新输入后。内部 `isReplay` 表示历史加载，不生成播放器文案、新的请求、通知消费或 Hook。

#### Scenario: Reply is absent from display cache
- **WHEN** Timeline 已收到 Agent 回复、可关联已记录通信请求/response，而 updates 没有该 receipt projection
- **THEN** full resume 将回复恢复在对应历史区间，后续已记录正文仍在其后，不集中追加到历史尾部。

#### Scenario: Reply already has a historical position
- **WHEN** display cache 已含同一 receipt，或者 reconnect 保留已显示记录
- **THEN** 保留其原位置并只展示一次，不由 load 后 snapshot 追加副本。

#### Scenario: Child history is opened after resume
- **WHEN** 用户在恢复父页面后首次打开 child
- **THEN** child 使用同一有序历史规则，已有 request/reply 的样式和身份与正常显示一致，不将通信单独移到末尾。

#### Scenario: Incoming inquiry cache omits a phase
- **WHEN** 接收方 inquiry 的缓存缺少 received、approval 或 completed 阶段，或不同 source peer 使用相同 inquiry ID
- **THEN** 仅按 source peer、inquiry ID 和 phase 补齐缺失事实，已有阶段原位保留并可作为经 Timeline 身份核对的锚点，不跨 peer 去重，不创建新 inquiry 或审批请求。

#### Scenario: Historical anchor or body is absent
- **WHEN** receipt 无可验证定位锚点，或已保存 payload 缺失/校验失败
- **THEN** 使用明确且有界的降级位置或既有缺失说明，不以文本、当前时间或执行历史来补齐，不伪造精确顺序。

#### Scenario: Reconnect cursor crosses synthesized history
- **WHEN** 本次恢复需合成不在 physical cache 中的历史记录，不能证明当前 cursor 已涵盖它
- **THEN** 按既有完整替换规则重建有序历史；合成记录不充当源缓存 cursor，重复 reload 不重复显示。
