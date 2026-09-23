# 子 Agent 工具装配、用途语义与单次审核

审查日期：2026-09-20。基线 HEAD：`3024dad1ef75a29b1e94a3b5e47083255ad5676e`，以当时工作树为准。工作树已有采样、Timeline、恢复及文档修改；本次保留原样，只增加审计记录。`actor/turn/sampling.rs` 已有未提交修改，因此本文不把当前读取结果全部归因于 HEAD。

正文中 `agent/`、`shell/`、`workspace/`、`tools/` 开头的源码路径均相对于 `crates/codegen/`；仅写文件名时沿用同段所属模块。

用户明确的设计意图：子 Agent 默认使用有匹配约束的编辑工具；对整文件覆盖等需要额外判断的操作，可以发现工具、理解限制，并把具体请求交给主 Agent 审核。

结论：现有系统已经实现硬 eligibility、初始 RWX、主上下文单次判断与执行 permit；缺口主要在“什么操作必须审核”的表达和审核输入保真。`write` 当前由运行时注入而不进入 authored eligibility，因此落入永久拒绝。简单将其加入 preset 又会使 ReadWrite/All 子 Agent 默认获得它，仍不能落实用户描述的策略。

## 契约与证据层次

- 已归档权威：`openspec/specs/tool-authorization/spec.md` 的 deny-before-approval、保守 shell RWX、delegated mode/MCP identity 上限；`openspec/specs/local-coordination/spec.md` 的父子询问与“询问回答不授予权限”。
- 架构说明：`crates/codegen/agent/PROMPT_ARCHITECTURE.md:45-77`、`docs/architecture/behavior-state-overview.md:108-116`。它们描述单次审核和 permit，但不能代替主规范中缺失的行为场景。
- 实现：Agent preset/builder、child spawn、SubagentCapabilityState、tool preparation、PermissionManager、primary judgment Sideband、dispatch、具体文件工具。
- 真实会话：`01a0bc7a-09bd-7843-9f4a-0e9e6a065f80` 的 capability catalog 与 seq 408–410、427。只引用工具授权事实，不复制业务文件正文。
- Atlas 已 project(open) 并执行局部 search/symbol/calls。方法查询出现未物化结果，结构行号也与当前源码不一致；最终位置与调用关系以当前源码直接核对为准，不宣称获得全仓精确调用图。

## 当前正常路径

这里存在两个不同的“两阶段”，需要分开理解。

装配阶段先得到 Agent 声明的工具，再补入 runtime 工具并应用最终限制；调用阶段则先冻结、判定和签发 permit，再在 dispatch 消耗和复验 permit。装配本身不发起主 Agent 审核。

| 事实 | 所有者与产生方式 | 实际含义 |
| --- | --- | --- |
| 工具用途、实现和最大 RWX | tool descriptor / 具体 Tool | 这是什么工具，以及单次调用可能具有的副作用上界 |
| authored 工具身份 | preset / additionalTools 等解析，child 构造前冻结 | 当前 Agent 哪些精确 native identity 可以进入授权流程 |
| 可见工具 | finalized ToolBridge | 模型可看到哪些 schema；可见不代表可执行 |
| native eligibility | authored identity 与最终 bridge 求交；另有只读便利和窄控制例外 | 超出此集合的调用在进入 PermissionManager 前拒绝 |
| initial RWX | child capabilityMode，经上级 delegated mode 求交 | eligible 调用是否需要额外的权限判断；不是可被一次批准扩大的会话 grant |
| permission mode | 独立的 child Ask / Auto / AlwaysApprove 配置 | 超出初始 RWX 的 eligible 调用走人工、主模型判断或自动放行；显式规则仍优先 |
| one-shot permit | Shell preparation | 绑定调用身份、参数哈希、cwd、RWX、actor epoch、MCP transport generation |

源码入口：`agent/src/config.rs:195`，`agent/src/builder.rs:587`，`shell/src/agent/subagent/handle_request.rs:1076`，`shell/src/session/actor/spawn.rs:1517`，`shell/src/session/subagent_capability.rs:78-222`。

调用流程：

1. `preparation.rs:493-579` 通过最终 bridge 解析参数，计算所需 RWX，检查 descriptor 和 hard eligibility。
2. eligible 且 initial RWX 覆盖时，进入普通权限路径；仍检查 deny、managed Ask、protected path 与 shell 风险，不是绕过 PermissionManager。
3. eligible 但 initial RWX 未覆盖时，根据 child permission mode 处理。Auto 的未被规则直接解决的调用进入 primary-context judgment；Ask 通常走用户交互；AlwaysApprove 按其配置语义放行。不是所有请求都会调用主模型。
4. Auto judgment 使用主会话 active model 和真实用户来源的任务证据，禁用工具，返回严格的 allow/deny + reason。它是独立 Sideband，不打断主任务，也不把回答变成普通会话权限。`ask_parent` 用于澄清，不安装 grant。
5. 允许后签发调用绑定的 permit；dispatch 再检查身份、参数、cwd、descriptor、child epoch 与 MCP generation，并一次性消耗。

审核所有者和执行边界：`workspace/src/permission/manager.rs:1887-2049`，`shell/src/session/actor/turn/sampling.rs:1034-1105,1140-1235`，`shell/src/session/actor/tool/authorization.rs:252`，`shell/src/session/actor/tool/dispatch.rs:38-120`。

## 已确认 gap

### G1 / P1：`write` 的可审核身份缺失，RWX 也不能表达其默认必审策略

触发条件：`grow-build` preset、runtime write fallback 开启、子 Agent 未额外声明 `write`。

`default_grow_build_toolset()` 有 `SearchReplaceTool` 而没有 `WriteTool`；child 在 builder 注入之前冻结 authored snapshot。builder 后加的 `write` 可见，但 `native_descriptor_is_eligible(false, Write, Write)` 为 false；preparation 在权限流程之前拒绝。这个行为符合当前实现及架构说明，却没有实现用户希望保留的审核通道。

反过来，把 `write` 加入 authored 集合也不足以修复：`SearchReplace` 投影为 ReadWrite，`Write` 投影为 Write。只要 initial mode 允许普通编辑，它就必然覆盖 Write。对没有额外 Ask/protected 条件的调用，PermissionManager 的 in-fence 分支直接允许。

需要表达的策略是“identity/操作要求额外审核”，它独立于 RWX 的读写执行分类。当前 `ToolFilter` 也只有 Edit，`Write(...)` 规则与 `Edit(...)` 一起解析成 Edit，无法用现有名称规则只约束整文件覆盖。

证据：`agent/src/config.rs:152-156,195-220`；`agent/src/builder.rs:606-614`；`subagent_capability.rs:78-119,214-222`；`tool/authorization.rs:122-124`；`tool/preparation.rs:558-579`；`workspace/src/permission/manager.rs:1887-1912`；`workspace/src/permission/rules.rs:172`。

验收方向：同一个 ReadWrite 子 Agent 的普通 `search_replace` 可直接使用，`write` 可发现但在执行前进入审核；拒绝不写文件，允许仅执行当前冻结调用，第二次调用重新判定。硬 deny 始终不可申请。

### G2 / P1：进入审核后，真实工具身份和完整调用证据被降级

`AccessKind::from_tool_call` 把 `SearchReplace`、`HashlineEdit`、`Write` 都转成 `Edit(path)`。PermissionManager 再通过 `tool_name_for_access` 把 Edit 固定命名为 `search_replace`，并只把路径放入 access_detail。这个名称同时进入分类器请求和权限审计，不仅是一个 UI label。

因此，对于一个已经 hard-eligible、因初始 RWX 不足而进入审核的 `write`，主模型的结构化请求可能是 `tool=search_replace, detail=path`。当前调用的原始 identity/content 可能出现在 recent_child_context，但那里每个 tool argument 只保留 400 字节左右的截断文本，且没有独立的完整冻结调用契约。长内容不能靠这个旁路可靠审核。

执行 permit 的确绑定了完整 canonical arguments，防止批准后调包；但这不能证明审批者看到了对应的真实操作。这里是“审批对象”和“执行对象”之间的信息缺口。

证据：`workspace/src/permission/types.rs:325-329`；`workspace/src/permission/prompter.rs:759-765`；`workspace/src/permission/manager.rs:1606-1631,2034-2049`；`workspace/src/permission/auto_mode.rs:305-312,1301,1430-1458`；`shell/src/session/actor/laziness_classifier.rs:492-510`。

验收方向：审核与 audit 保留 actual wire identity、操作类型和冻结参数身份；对覆盖操作提供明确的目标、覆盖语义和有界但充分的改动证据，超预算时不能把关键缺失伪装成完整。执行继续复用现有 permit，不能建立另一套会话授权。

### G3 / P2：能力目录缺少逐工具的限制原因和对应通道

当前目录能区分 available、call-projected、forbidden，这是已有能力。它只保存 `(kind, max_access)` 与 eligible map，没有记录“为何排除”的来源。因此 `write`、深度限制的 spawn、Goal ownership 等不同原因会聚合在同一个 `never permit-able` 列表。

preflight 的拒绝文本也把 authored exclusion 和 MCP transport 失效合并，不能告诉模型当前究竟是工具用途限制、配置禁用、父级上限、深度限制还是 transport 变化。builder 直接裁掉的 disallowed 工具甚至不会进入 visible map。当前可见性是最终 bridge 的投影，不是完整的受限能力目录。

证据：`subagent_capability.rs:56-68,284-332`；`tool/preparation.rs:568-572`；`agent/src/builder.rs:732-748`。

验收方向：从同一份最终策略结果生成状态、原因、是否可申请与申请方法。可审核工具说明“提交这次精确调用，由 Gate 审核”；永久禁止说明不可升级原因。无需为了可解释性暴露全部未注册工具 schema。

### G4 / P2：审核的产品语义没有完整进入主规范

`tool-authorization` 当前主要覆盖 deny、shell 投影和 nested RWX/MCP ceiling。三态目录、authored/runtime 边界、逐工具额外审核、审核输入必须保持实际 identity、one-shot permit 与恢复失效没有完整的 WHEN/THEN 场景。

具体实现和测试比主规范丰富，架构说明又容易把“所有后来层只能缩小权限”“initial RWX ceiling”“单次越界批准”混读为同一概念。当前语义应该明确：初始 RWX 是免额外判定的范围；hard eligibility 是可审核边界；permit 是某次调用的事实。

还需明确 Ask / Auto / AlwaysApprove 与“必须主代理审核”的优先级。现有 AlwaysApprove、显式 Allow、已有 session grant 可以在 classifier 前解决请求；单次 permit 也不等于每次都必须重新询问主模型。不能只改变目录文案却保留不同的执行策略。

验收方向：后续行为 change 先固化上述场景，同步开发说明，测试通过后归档。当前审计不提前修改主规范。

## 与重要文件保护有关的独立债务

### D1：匹配式编辑降低误覆盖概率，但目前不是原子并发保护

`search_replace` 新文件路径先 read 再 write；任何读取错误都按不存在处理，之后调用无条件 `write_file`。所以读取失败但可写、检查后其他写者创建文件，都可能越过“空 old_string 不覆盖已有非空文件”的保护。普通替换也是读取当前全文、匹配、再写回全文；在 read→write 之间的并发更新可能丢失。

批内路径锁只在当前 `execute_parallel_tools` 调用构造，按原始参数路径分桶，不覆盖不同 child、外部编辑器和路径别名。`write` 也没有 expected-version 参数；permit 绑定调用参数但不绑定文件原始版本。

证据：`tools/src/implementations/grow_build/search_replace/mod.rs:245-285,478,651-653`；`tools/src/implementations/grow_build/write/mod.rs:24-31,107-132`；`shell/src/session/actor/tool/mod.rs:334-350,397-420`。

这项与授权语义分开修复：区分 NotFound 与读取失败；新建采用不覆盖语义；已有文件编辑定义预期版本/冲突结果与实际 filesystem 提交边界。不能把仅对协作写者有效的 mutex 或普通 rename 宣称成跨进程 CAS。本次是静态确认，未做真实并发或故障注入。

### D2：native 子代继承上限需要明确语义

`DelegableCapabilityCeiling` 当前保存 initial mode 与 MCP client binding，没有 native identity 集合；后代根据自己的 Agent 定义构造 native eligibility。已归档规范明确验证 RWX/MCP 交集，没有承诺祖先所有 native identity 都逐级求交。

因此不能笼统宣称“父 Agent 不能调用的任何工具，后代都不能调用”，也不能在未核对显式角色委派意图前直接称其为漏洞。如果新的 `write` 必审策略属于祖先不可削弱的约束，必须验证它不会因选择另一个 child preset 而消失。此项作为策略边界待明确，未做端到端越权复现。

## 反证与前序结论修正

1. `authored_capability_tools` 并没有同时保存 initial grant。`initial_mode` 与 `eligible_native` 已分别存在，前序“一个字段混用两个语义”的结论不准确。
2. 主 Agent 审核并未缺失。Auto 的 primary-context judgment 和工具绑定 permit 都已有实现；不需要新增通用 capability request/grant 协议。
3. `ask_parent` 回答不是授权，这与已归档规范一致。
4. 原会话 seq 408 尝试 `write`，seq 409 明确返回 eligibility 拒绝；seq 410 虽然 `state=completed`，但 `outcome=not_dispatched`、`details.dispatched=false`。前序把 completed 当成成功执行的推断已撤回。本样本证明 preflight 拦截生效。
5. 后续 seq 427 改用 `search_replace` 与真实限制一致；重新读取第三版 `types.py` 是并发工作区上下文变化，不是授权升级事件。

## 建议的最小演进顺序

1. 先定义工具用途对应的判定策略：普通匹配编辑、需要审核的整文件覆盖、硬禁止；把额外审核要求放在现有工具/Agent 策略解析结果中，与 RWX 正交。避免只给 `write` 提高虚假的 RWX。
2. 补齐权限请求的 actual identity 和操作证据，保留现有主上下文 Sideband、deny 优先和一次性 permit。
3. 由同一判定结果投影目录、拒绝原因、审核请求和 audit；不让模型从 runtime 注入顺序猜策略。
4. 以真实 builder→child→preparation→judge→dispatch 回归验证三态，同时覆盖默认 preset、额外声明、feature 关闭、不同初始 RWX、permission mode、Agent 切换和 permit 失效。
5. 文件冲突保护与后代策略继承独立立项，不混入本次审计或靠扩大授权顺带处理。

建议保留当前 exact-call 方式：子 Agent 提交具体操作，主 Agent 判断后只允许该操作。路径级或 session 级能力租约不是当前需求所必需，也会扩大状态和恢复负担。

测试覆盖和实际执行结果见本 change 的 `verification.md` 对应日期段落。
