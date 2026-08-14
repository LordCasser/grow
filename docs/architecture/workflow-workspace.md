# Workflow Workspace 架构

> **Status**: Implemented
> **Date**: 2026-08-12
> **Scope**: shell / workflow / pager

Workflow 由四个所有权边界组成，而不是一段“可变的后台脚本”：

1. **Workflow Behavior** 是公共 Workflow 的唯一协作协议。只有 turn 捕获的 Behavior 与
   实时 Behavior 都是 Workflow 时，Grow 才能搜索、创建、编辑、验证、发布、启动或管理
   公共 Workflow。Deep Research 使用独立的私有 runtime。
2. **Workflow Workspace** 归 session 所有，持久化多个草稿、唯一 Definition 焦点、派生
   来源、基线、当前内容哈希、验证哈希、保存提示和发布冲突。它不持有运行中执行器。
3. **Workflow Definition** 是可搜索、可编辑和可复用的 Rhai 定义，scope 为 Session、
   Project、User 或 Builtin。修改已保存 Definition 时先派生 session 草稿；发布要求显式
   选择 Project 或 User，并使用原子写入与基线哈希检查。
4. **Workflow Run** 是 Definition 内容与启动参数的不可变快照。`WorkflowManager`、
   `WorkflowTracker` 和 `WorkflowRunStore` 分别拥有执行、状态和持久化；同一 Definition
   可同时产生多个独立 Run，句柄按 `name`、`name-2` 递增。

Rhai 引擎仍然是确定性执行层：脚本禁用运行时求值和睡眠，host 调用按序列与哈希记录
journal；`agent()`、`parallel()`、`phase()`、`complete()`、`pause()`、`await_user()` 和
`budget()` 构成编排接口。Definition 的改变只影响下一次 Run，暂停/恢复也始终使用原始
Run 快照，而不是重新解析当前 Definition。完整的脚本写作契约（meta、函数签名、Agent
选项、限制与最小示例）见 [workflow-rhai.md](../workflow-rhai.md)。

## 发现与编辑

未指定 Definition 时，主 Agent 先判断焦点是否与请求相关，再按 `name`、`description` 和
`when_to_use` 搜索 session、project、user、builtin 元数据。唯一明确匹配可说明来源和参数
后直接使用；歧义候选必须让用户选择。只有参数变化时复用 Definition；阶段、编排或 Agent
提示变化时派生草稿；没有候选时才新建草稿。

Grow 只原生编辑 session 草稿。Project/User Definition 文件对 Grow 是发布目标而不是编辑
目标：即使已处于 Workflow Behavior，也必须先派生、修改并验证草稿，再通过 `publish`
原子替换。外部编辑器仍可直接维护保存文件，Registry 会在下一次扫描时重新发现并报告诊断。

Registry 快照同时返回有效 Definition 与结构化诊断。错误文件不会静默消失；语法错误、
meta/文件名不一致、不可信路径、同 scope 重名和发布冲突都保留来源与错误码。

## 用户界面

Workflow 外只提供 `/workflow [prompt]` 与 `/behavior workflow` 快速入口。Workflow 内提供
`/workflows`、`/workflow-run` 和可运行 Definition 的动态命令。`/workflows` 分区显示
Definitions 与 Runs：前者显示焦点、scope、临时/已保存、dirty、validated、conflicted；
后者显示句柄、Definition scope/hash、状态、阶段与 Agent 进度。

有 Active 公共 Run 时离开 Workflow 需要重复确认，Run 继续后台执行；重新进入 Workflow
后才能管理。暂停、预算受限和未保存草稿不会阻止切换。Deep Research 不出现在公共列表、
动态命令或管理入口中；其运行状态仍通过独立的 Pager 显示通道可见（transcript 进度块、
tasks pane 的 Deep Research 状态行与 activity projection），这些显示面只消费 shell 发布的
`WorkflowUpdated`，管理面数据（`workflow_runs`）不受私有 run 影响。
