## Why

用户明确第2项目标为主Agent控制运行中的subagent，切换其model/effort。此前set_session_model绑定调用者自身，虽然经验证，目标语义不满足用户要求。本change替换该入口，不把原验证当新特性的完成证据。

## What Changes

- 原生Grow工具改为set_subagent_model，必填subagent_id，model/effort至少一项；拒绝任意session ID、client intent和未知字段。无兼容别名或自身控制入口。
- source由已有session-bound SubagentBackend注入，coordinator按immutable immediate parent与active执行资格转发到确切child runtime，非直属/未启动/终态/取消/挂起/Workflow拒绝。
- Shell沿原catalog锁、用户选择准备、actor admission ACK、target闭合Step/durable应用；返回queued只表示接纳。父route与全部能力授权保持。

## Capabilities

### Modified Capabilities

- model-sampling：运行时控制指定直属child，model-only用户默认/effort-only目标最新desired。
- tool-authorization：source绑定宿主，target经coordinator动态资格和exact runtime身份验证，普通工具gate不可绕过。

## Impact

涉及tools native schema/注册/Task backend与coordinator、Shell runner/共享model handler/清理旧self binder、相关单测/实际provider集成、主规范与使用说明。用户既有resume字段覆盖及checkpoint约束保持；测试全部隔离GROW_HOME，不安装binary或干扰正常运行。
