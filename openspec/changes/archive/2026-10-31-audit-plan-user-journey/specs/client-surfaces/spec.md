## ADDED Requirements

### Requirement: Plan actions remain discoverable across layout and focus changes
Plan 审批与预览 SHALL 根据实际可用宽度排列操作提示与点击区域，内容 viewport SHALL 扣除真实操作栏高度。只要窗口仍可显示计划详情，不能仅因单行宽度不足就隐藏整排操作。收起、Preview、Prompt、Commenting 和 pending confirmation 的提示 SHALL 对应真实可用按键；Fullscreen 和 Minimal SHALL 不把空 Enter 描述为批准，不把放弃描述为继续计划。

#### Scenario: Approval with comments in a narrow terminal
- **WHEN** 审批窗口包含批注，终端宽度不足以把所有动作放在一行
- **THEN** 动作换行显示，批准、要求修改等关键动作保留可见及正确点击区域，计划内容不被 footer 覆盖。

#### Scenario: Collapse and reopen approval
- **WHEN** 用户收起待审批计划后操作输入框或重新打开预览
- **THEN** 显示与当前路由一致的修订/重开提示，不虚构可用按键，不批准或放弃计划。

#### Scenario: Minimal revision prompt and pending decision
- **WHEN** Minimal 中修订输入为空，或决定正在等待确认
- **THEN** 空 Enter 不能被提示为批准；pending 界面只展示实际可用的核对/收起操作，不展示可再次批准或修订。

#### Scenario: Minimal ordinary and retained Plan preview
- **WHEN** Minimal 用户打开普通计划或 retained candidate 的详情
- **THEN** 实际接收输入的计划预览可见，retained candidate 的 Review 入口可见且可用，收起后恢复正常输入。

#### Scenario: Minimal line comment editing
- **WHEN** Minimal 用户进入计划行内批注编辑
- **THEN** 编辑框与输入内容可见，Enter 保存批注，Esc 取消本次编辑并保留此前修订草稿。

### Requirement: Casual Plan comments disclose ordinary prompt delivery
不存在 live approval 时，计划预览发送批注 SHALL 明确这是普通消息，正在执行时遵守普通输入排队规则；界面 SHALL NOT 将它描述成已接纳的审批修订。保留计划仍使用 Review 入口进入正式审批，批注继续按提交身份绑定。

#### Scenario: Send comments from an ordinary preview
- **WHEN** 用户从普通计划预览发送批注
- **THEN** 提示说明普通消息发送或排队，不宣称计划已进入修订或决定已接纳。
