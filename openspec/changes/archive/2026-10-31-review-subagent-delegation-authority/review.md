# Subagent 委派权限复审

2026-10-31，基于 HEAD `e4d82ebd6` 加当前工作区改动，包含上一轮错误归因与 toggle 热更新修复。此次是审查，不实施下述修复。路径行号均按审查时工作区。

## R1 · P1：按请求字符串授权，同一 Agent 的另一种写法可绕过禁用或 deny

`crates/codegen/shell/src/agent/subagent/resolution/definition.rs:129` 的 toggle gate 直接查询 `toggles[subagent_type]`；`agent/subagent/mod.rs:1871` 同样用请求字符串查父 filter。定义解析却允许多个字符串命中同一文件：

- 普通项目定义 `audit-worker` 与 `./audit-worker`：`crates/codegen/agent/src/discovery.rs:578-616` 将输入拼为文件路径，并把请求字符串重新写入 `def.name`。
- 插件定义 `audit-plugin:audit-worker` 与 `audit-worker`：`discovery.rs:487-503` 支持唯一插件的 bare fallback。

当父策略没有非空 allow 白名单时，设置 `toggle[正式 ID]=false`，正式 ID 返回 Disabled，替代写法返回 Ok；仅设置 `subagents.deny=[正式 ID]` 也有相同绕过。普通定义探针另行核对两个 source_path 的 canonicalize 结果相同，插件探针核对 selector_identity 相同。预检与实际派发共用该 gate，因此不是单侧文案遗漏。严格 allow 白名单不包含替代写法时仍会阻止它；此发现不是任意 allow 白名单绕过。

影响是委派目标限制失效；不会因此绕开子会话的后续工具 Gate 或 RWX/MCP ceiling。

修复应先得到稳定的定义身份，再对该身份做 toggle 和 parent allow/deny 判定，目录和派发共用同一结果。普通 path ID 应拒绝或统一处理 `.` 等非规范写法，不能把任意请求路径重新当作注册 ID；插件 bare fallback 若保留，必须授权到正式限定 ID。对 native/plugin 两组分别保留禁用、deny、allow 与同名冲突回归。

## R2 · P1：挂起恢复重新计算出生 RWX，可能扩大免逐调用审批能力与后代上限

`handle_request.rs:430-469` 在恢复时从 Control 取当前选中的 Agent 定义，再以原请求 overrides 调用 `resolve_runtime_config`。原请求没有明确 capability_mode 时，`resolution/definition.rs:272-275` 重新采用该定义当前默认值。

出生时已算出的实际模式保存在 Spawn 的 `capability_mode`（`handle_request.rs:1043`），但 `execution_recipe` 保存的仍是原请求的可选 override（同文件 `:1037`；`tools/.../task/types.rs:178-186`）。恢复入口 `agent/mvp_agent/runtime_handoff.rs:705-723` 只恢复 recipe.runtime_overrides，没有把原 Spawn 的有效模式带回。`handle_request.rs:478-483` 只与直接父 ceiling 取交集；根主会话没有该 ceiling。最后 `session/actor/spawn.rs:1644-1664` 根据重算的 definition 新建 child authority 和后代 ceiling。

可达条件：第一次 Task 省略 capability_mode，从定义默认得到 ReadOnly；之后用户切换到默认 All 的 profile（运行中正常保持原 ReadOnly），或者离线把原定义默认改成 All；挂起的同一 child 重建后得到 All。显式请求 ReadOnly 或受只读直接父 ceiling 限制的情况不会发生这个扩大。

这与“新 Task 的 resume_from 创建新子会话”不同：这里是原 lifecycle 的 checkpoint restore。局部探针验证 recipe 丢失有效默认值以及 resolver/ceiling 的 ReadOnly→All 数据流，源码链路确认恢复入口使用这些值；未执行带真实 provider 的完整挂起/恢复。

修复应以已持久化的出生能力事实恢复不可变上限，并与当前合法上限相交；当前 profile 可以改变工具资格，不能重新授予出生 RWX。一次性 permit 仍应丢弃并重新校验。MCP binding 的跨恢复语义需要在同一修复中核对，当前不将未复现的 MCP 扩权列为独立结论。

## R3 · P2：禁用插件 Agent 后目录仍列出它

`discovery.rs:361-420` 先调用 `merge_subagents(discovered, toggle)` 过滤 native 定义，随后追加插件条目，没有检查对应 toggle。因此即使完全使用正式 `plugin:agent` ID、不触发 R1，运行时目录仍会列出已禁用插件。

`AgentBuilder` 的 `has_subagents` 和 Task 说明依赖此目录（`builder.rs:648-698`）；若父 Agent 只允许该插件，关闭它后 Task 仍在，而派发必定返回 Disabled。父拒绝诊断的 available 列表也可能继续包含它。热更新重新构建目录仍复现，因此上一轮 native-only 回归不足以覆盖插件。

修复应在合并所有来源后统一应用 toggle/filter，覆盖仅剩一个插件、全部关闭、重新开启和错误候选列表。

## R4 · P2：CLI Agent 可通过派发校验，却不进入普通 Task 目录

headless `--agents` 在 `crates/codegen/pager/src/headless.rs:792-794` 填充 `Config.cli_agents`。执行解析将其作为 fallback（`resolution/definition.rs:60-75`），校验也接受 CLI 名称。但普通 `AgentRebuildSpec` 没有这份定义输入，`AgentBuilder` 只枚举文件/builtin/plugin（`builder.rs:648-665`）；spawn 中同一 CLI 列表只传给 Workflow catalog source。

当父 allow 只含一个 CLI-only 类型时，gate 返回 Ok，真实 build_agent 却因目录为空移除 Task。若还允许内置类型，则 Task 保留但说明遗漏 CLI 类型。修改 toggle 无法补上未传入的来源。

修复应让普通目录和执行解析共享定义来源及优先级，不为 CLI 额外建一套授权规则；验证 CLI-only、与文件同名、禁用及 hot reload 后的一致性。

## 已核对的边界

| 层次 | 当前约束 | 本轮观察 |
| --- | --- | --- |
| 委派资格 | enabled、Task 资格、深度、定义存在、toggle、父 allow/deny | 身份未统一，见 R1；R3/R4 是目录与执行不一致 |
| 新 child 初始能力 | Task 的能力请求经调用 Gate，后代与直接父出生 ceiling 取交集 | 运行期交集正常；恢复需补原 child 自身的出生事实，见 R2 |
| 单次工具调用 | exact identity 硬资格 → 所需 RWX → 普通权限 Gate | read-only 并非永远禁止一切写入；硬资格内超出初始 RWX 的调用可经配置的逐次审批获得 permit |
| 显式规则 | deny 在 always-approve 和 child in-fence 快路径之前 | 既有集成测试通过；always-approve 不替代委派资格 |
| MCP | 继承池、server/client identity、当前 Agent 投影及 live generation | 既有切换/资格测试通过，未发现本轮可证实的 live 绕过 |
| Agent 切换 | 可变工具/profile 与不可变出生 RWX 分开 | 运行中的切换测试通过，问题出在恢复时重建 authority |
| 派发 permit | 参数 hash、cwd、actor epoch、MCP generation 和一次消费 | 既有 permit 测试通过 |
| 生命周期 | 挂起树与新 resume_from 不同，Workflow 使用 frozen route | 不把 Workflow 冻结策略或新任务重新授权误报为 live toggle 绕过 |

`apply_immediate_delegation_context` 在异步读取资源前保存父 profile。因“已准入委派是否随父 profile 切换撤销”的边界尚未证实违约，本轮不把该时序列为确定 bug。缺失父会话的预检仍使用默认上下文，但 runner 会拒绝实际派发；没有证据将其定为权限提升。

建议顺序：先收敛定义身份并修 R1，再固定 restore 出生权限（R2），最后统一目录来源及过滤（R3/R4）。保留现有 Gate、ceiling 和控制队列，不新增平行权限框架。
