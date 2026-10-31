## ADDED Requirements

### Requirement: Accepted Plan decisions release confirmation UI without polling
Plan 决定的 Control durable ACK 成功后，Shell SHALL 通过既有模式更新交付本次决定回执。Pager SHALL 按 session/binding、submission、decision identity、action 和 feedback 精确核对后释放对应确认界面。早到状态查询的 unconfirmed SHALL NOT 使后续已接纳决定永久停留在 check confirmation；普通 phase 更新或写入失败 SHALL NOT 被当作确认。

#### Scenario: Status query overtakes decision acceptance
- **WHEN** 客户端首次状态查询返回 unconfirmed，而后端随后持久接纳该决定
- **THEN** 对应回执使客户端自动结束该确认界面，无需用户按键查询或新增轮询。

#### Scenario: Durable admission fails
- **WHEN** 决定的 Control 写入未获成功 ACK
- **THEN** 不发布已接纳回执，客户端保留决定与反馈等待核对，不宣称成功。

#### Scenario: Receipt does not match the pending decision
- **WHEN** 回执的 session/binding、submission、decision、action 或 feedback 与待确认决定不同
- **THEN** 回执不能关闭或清除当前审批与批注。

#### Scenario: New submission supersedes an old confirmation view
- **WHEN** 新权威审批替换旧提交，旧决定仍在客户端等待确认
- **THEN** 新审批可以正常批注和要求修改，不被旧 pending 锁住；旧批注保留原提交身份，迟到旧回执不关闭新审批。

#### Scenario: Same submission is presented again
- **WHEN** 未确认决定对应的同一提交再次显示
- **THEN** 原决定仍保持确认中，不能因重绘或重发而重复提交冲突决定。
