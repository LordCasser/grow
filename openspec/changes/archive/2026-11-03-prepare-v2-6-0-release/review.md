# v2.6.0 候选审查

基线是 v2.5.5 / HEAD `bbd2c4dfa`，对象是本地候选工作树（大量既有未提交改动及新源码、测试、归档）。用户确认使用 2.6.0。审查不以归档任务已勾选替代当前源码验证；也不自动提交整份工作区。

## 已确认并处理的发现

1. **P1：生产 prompt 遗漏子任务观察指引。** `AgentBuilder::build` 以 `PromptContext::render()` 生成稳定 head；该入口使用空工具集合。原 `audience/primary.md` 中的 `tools.by_kind.subagent_*` 条件因此恒不出现，直接以完整 renderer 渲染原模板的测试没有覆盖生产组合。已在独立 `2026-11-03-fix-agent-observation-prompt-layer` 归档 change 将指引放入现有 session extensions，保留稳定 head 不依赖工具集，并从生产 Context 组合和实际 Builder 验证 Full/Extend、主/子 audience、重命名和缺工具。
2. **P1：Dashboard 操作指南与删除入口冲突。** 03/04/17/23 指南仍含双按 Ctrl+X 永久删除、预览内回复或 `/quit` 关闭当前 session 等旧说明。实际 `SessionCommands`、AgentView 和 root dispatch 使用 C 关闭、D 独立确认、重复前缀仅 stop；总览预览只读。已同步 README、用户指南及开发说明。前缀本身仍有两秒选择期限（`LEADER_KEY_TIMEOUT`）；D 后的删除确认与它独立，未改实现。
3. **P2：升级限制没有随新版本说明交付。** 新 Spawn 记录出生 MCP 成员，旧挂起 child 缺事实时 cold restore 明确拒绝；只读历史仍可查看。已明确写入 2.6.0 notes，没有声称兼容迁移或补造旧授权。
4. **P2：恢复与协调说明失真。** Task 描述声称继承 tool state，而实现重建 runtime/tools、不会复制 permit；guide 的 `plan` builtin 已不存在，用户同名定义不能覆盖 builtin；成功 inquiry 已支持目标端 passive context。已按源码与现有契约修正。
5. **P2：快捷键与引用陈旧。** `Ctrl+.` 未注册，`?` 打开 command palette，`/shortcuts` 打开独立帮助；四条本地 Markdown 引用目标缺失。已修正并重新核对活跃文档链接。
6. **P1：已提交 tool meta schema 未覆盖新增 ToolKind。** `tool_meta_schema_is_up_to_date` 在完整 tools 套件中失败。已按现有 `UPDATE_TOOL_META_SCHEMA=1` 入口重生成 JSON Schema 并对齐示例，未删除断言或新增兼容词汇。
7. **P1：闭合元数据词汇改变却沿用版本 5。** v2.5.5 的 `TOOL_META_VERSION` 已为 5，三种新 subagent kind 必须让 typed consumer 识别版本变化。已另立 `version-subagent-tool-metadata` change，通过统一构造入口提升为 6，同步 schema、发布说明和真实序列化回归，不改变权限或 Timeline 版本。

## 未解除的发布阻塞

- **MCP/文件监听：** 本机 Rust 1.93.1 下 Pager 全组 7555 passed / 11 failed / 15 ignored；11 项集中于 MCP 热更新、目录变化和 cold load。Shell 全组另有三项 config watcher 事件超时。独立串行 watcher 组仍有 `config_watch_late_directory_and_recreation` 失败，不能仅归结为并发负载。
- **原树权限恢复：** `suspended_child_cold_restore_retains_birth_authority_end_to_end` 在全组及独立运行中均于 `authority.cold_load` 的十秒 fixture 期限失败；当前没有证明旧出生授权被放大，也没有足够证据宣称恢复正确，正式门禁仍未通过。
- 一次独立 MCP 用例通过不抹去全组失败。临时日志与 native sample 定位到 `spawn.rs:489 → ConfigFileWatcher::apply_session_update → watch_registered_path → notify::fsevent::FsEventWatcher::watch_inner/run → Receiver::recv` 在 ACP LocalSet 同步执行；这证明该线程存在阻塞调用，不足以断言它解释全部 15 个失败。临时 tracing 修改已移除。
- 这些发现涉及既有 runtime 所有权与平台监听边界，按工程约束登记 backlog 单独处理。本 change 不把它们混入 prompt/doc 修复，也不增加 sleep、放宽超时、跳过断言或宣布全量回归通过。v2.6.0 目前是有阻塞的候选准备，不是可发布状态。

## 跨模块核对

| 范围 | 核对依据与失败路径 |
| --- | --- |
| Tool/prompt 边界 | registry 的 Task 与 command 生命周期依赖、各 ToolKind/schema、finalized renderer、实际 Builder role；不再用 command output 获取 child 结果。 |
| Child 观察与结果 | `subagent/{mod,status,wait}.rs`、来源绑定的 `shell/agent/subagent/observation.rs` 与 coordinator/backend；核对共享绝对期限、root/direct parent、隐藏 Workflow、未知/终态和完成通知收据。 |
| 会话关闭/删除/恢复 | root dispatch 与 runtime lifecycle guard；核对目标绑定 epoch、ACK 后退休、独立 fork 与 canonical Result 引用、failed restore owner。 |
| 上下文与执行控制 | interjection、notification drain、inquiry passive delivery 和关联回归；上下文补充不得成为人类授权或任意重启前台。 |
| 图片与辅助路由 | Recap/Flush/Sideband 的 Surface 来源、辅助模型 endpoint/backend/credential 同源、出生 MCP 与当前 ceiling；核对已归档场景及真实入口测试。 |
| 算法与显示 | shared grapheme wrapping、watch selection、hunk line tables、search prefix 与历史修复；保留原作者差分测量的局部范围，不宣称整体性能提升。 |
| 发布 | workspace/Cargo.lock、changelog、Rust 1.93.1、release workflows、exact-test runner 与 immutable-source 门禁。 |

## 验证范围与发布边界

实际命令、失败和最终结果统一见 verification.md。静态核对与渲染测试不证明模型遵循率；本机测试不代表 Windows/Linux/OHOS 原生发布通过。

活动的 MCP elicitation 提案仍是明确暂缓项，不写成已实现功能。`analyze-command-resource-budget-and-cleanup` 是另一份未完成调查，其临时 probe 与资源治理方案不在本次 prompt/doc 修复范围；本轮若观察到失败会如实记录，不能删测试或冒称通过。

正式发布仍须最终 clean commit、annotated v2.6.0 tag 和绑定该 SHA 的官方 required gates/十平台构建/资产校验。dirty 本地构建不是正式发布，未执行 tag/push/publish。
