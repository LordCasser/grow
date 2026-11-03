# 分析验证记录

## 本轮完成的核对

- 阅读 OpenSpec session-timeline 的 exact source、树挂起/恢复、引用保护与 client-surfaces 的加载失败/退出契约；当前 MRTR change 与本次分析不重叠。
- 阅读已归档的 fork 多 session 审计、本机规则漂移隔离诊断及用户自行恢复后的实际确认。严格区分主 session 已恢复、fork 尚未观察到执行型恢复。
- Atlas 限定查询定位 source 方法后回读当前源码；没有执行全仓索引。
- 逐项核对 runtime source JSON、AgentDefinition 序列化字段、项目规则模型投影与 classifier setter、load 的 resident/cold 分支、Prepare/RetireSuspendedRuntime、partial barrier、已释放 checkpoint 的处理。
- 核对 SessionControlSnapshot、SubagentExecutionRecipe 与 WorkflowAgentDefinitionSnapshot 的信息范围，确认不能把前两者当成完整出生授权快照。
- 通过只读 luna 子任务盘点现有 source/restore/barrier/load/ProjectInstructions/classifier 回归，主分析回读关键实现，形成 `validation-plan.md` 的 18 组目标验收。没有以测试名推断未验证实现正确。

## 当前代码证据入口

| 位置（相对仓库根） | 支持的判断 |
| --- | --- |
| `crates/codegen/shell/src/session/actor/runtime_source.rs:7` | source JSON 包含哪些字段，项目规则来自 Agent 缓存，client Hook 使用 Debug 表示。 |
| `crates/codegen/shell/src/session/actor/suspension.rs:434` | prepare 按 operation/instance 和聚合 source hash 拒绝，之后才重建 continuation。 |
| `crates/codegen/shell/src/session/actor/spawn.rs:1725` | 挂起恢复不新注入规则，但 classifier 在 :1750 读取当前 Agent 的规则。 |
| `crates/codegen/shell/src/session/actor/prompt_build.rs:24` | typed ProjectInstructions 来源识别可复用。 |
| `crates/codegen/shell/src/session/actor/laziness_classifier.rs:535` | classifier framing 与 root-owned manager 边界。 |
| `crates/codegen/agent/src/config.rs:438` | 全 manifest 包括 color；operator clamp 等跳过序列化。 |
| `crates/codegen/sampling-types/src/types.rs:932` | 实际 route/cache key 含 model/backend/endpoint/query；不含 API key。 |
| `crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs:634` | load lifecycle gate、fresh disk read、resident actor 复用及 actor 注册后 restore 失败路径。 |
| `crates/codegen/shell/src/agent/mvp_agent/runtime_handoff.rs:561` | 已有原树恢复、partial barrier 补齐与既有恢复资格复用。 |
| `crates/codegen/shell/src/session/actor/suspension.rs:749` | 现有退休命令只接受 settled Suspended，不直接适用于 Restoring。 |
| `crates/codegen/shell/src/session/control.rs:22` | 普通 ControlSnapshot 的实际字段范围。 |
| `crates/codegen/tools/src/implementations/grow_build/task/types.rs:152` | child recipe 的实际字段范围。 |
| `crates/codegen/shell/src/session/workflow/tracker.rs:434` | 完整 Workflow Agent 执行源快照的既有 capture/restore，可评估复用。 |

## 限制

本轮只有分析授权。没有产品代码、主规范、用户配置或真实 session 数据改动，没有发送恢复/关闭/删除 RPC，没有重启用户进程。没有生成 Cargo 产物，没有新增实跑测试结果。前轮 408 项既有回归与 7 项审计失败仍只证明当时的审计结论，不证明本设计已实现。

S1 的推荐基于主 session 隔离证据和本轮源码链。S2 的权限收紧、模型控制、Hook/Workflow 依赖策略需要对应 delta 和实施测试。source hash 未直接序列化 operator clamps 不等于已证明权限绕过；这里只说明聚合相等不能替代实际权限证明。R1 仍未运行双客户端验证。

## 规范检查

归档前 `openspec validate --all --strict --no-interactive`：17 passed，0 failed。归档后全量 strict：16 passed，0 failed；archive strict：766 passed，0 failed。收尾任务在归档后完成检查并勾选，随后重跑 archive 校验通过。`git diff --check` 通过，分析记录内的相对链接均解析到已有归档文件。Backlog 已指向归档后的方案和验证计划。
