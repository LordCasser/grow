## ADDED Requirements

### Requirement: External pager feedback preserves request origin
外部分页器请求 SHALL 在创建到挂起重试和完成之间保留原 agent/session 来源，失败和等待反馈不得写入后来切换到的其他会话正文。

#### Scenario: View changes before pager completion
- **WHEN** 原 root 或 child 会话请求分页器后用户切换视图，随后发生等待或失败
- **THEN** 会话通知绑定原匹配来源，其他会话正文不接收该通知。

#### Scenario: Origin is removed or rebound
- **WHEN** 反馈到达时原来源已移除或会话身份不再匹配
- **THEN** 不把通知写入新会话，通过应用级反馈保持失败可见。

#### Scenario: Suspend retry preserves the request
- **WHEN** 终端挂起超时并重试分页器
- **THEN** 文件所有权、ANSI 设置与原来源一同保留，重试不改用当前活动视图。
