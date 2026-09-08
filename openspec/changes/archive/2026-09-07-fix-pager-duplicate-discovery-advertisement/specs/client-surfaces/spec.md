## ADDED Requirements

### Requirement: Pager skill discovery delegates advertisement to reload
Pager 收到 Skills discovery 事件 SHALL 先维护目录监听，再请求技能重读；同一消费分支 SHALL 不因新目录注册额外请求独立命令发布，发布由重读完成路径负责。

#### Scenario: 新技能目录
- **WHEN** Pager 收到 Skills 事件并补挂新目录
- **THEN** 请求 ReloadSkills，不额外发送 AdvertiseCommands。

#### Scenario: 仅 workflow 修改
- **WHEN** Pager 收到 Workflows 事件
- **THEN** 维护监听后仍直接请求 AdvertiseCommands。
