# 实施前 prompt 与修改分析

核对日期：2026-09-22。HEAD 为 `3024dad1ef75a29b1e94a3b5e47083255ad5676e`，实际基线是实施前工作树。`audience/subagent.md`、`agent/src/builder.rs`、`PROMPT_ARCHITECTURE.md` 等已有其他任务修改，不能将它们归因于本 change。本文件保留逐处修改依据，行号定位实施前版本；后续授权实施的区段与保留校验见 [verification.md](verification.md)。

## 实际装配路径

| 层 | 当前入口 | 这次如何处理 |
| --- | --- | --- |
| 稳定 System head | `agent/src/prompt/context.rs:115`，`PromptContext::render_with_renderer` 拼接 Mandatory Core 与一个 Audience | 只替换 Audience 中对应的职责正文，保留稳定头的结构 |
| Mandatory Core | `agent/prompts/foundation/mandatory-core.md` | 指令优先级、安全、实际工具可用性、项目规则作用域不改 |
| Agent role | `PromptContext::render_role_with_renderer`；Extend 为 standard + role + extensions，Full 不含 standard | 只调整两个内置 role 的专属指导，不把父级调度塞到共享 standard |
| 内置 Agent 定义 | `agent/prompts/agents/*.md`；生产 child 的 `discover_agent_definition` 使用正常发现路径 | 以实际 Markdown 定义为当前角色正文，不假定 common 常量就是生产入口 |
| primary 的 task 描述 | `agent/src/builder.rs:681` → `build_task_description` → `common/tool-types/src/task.rs:777` | 在现有说明追加委派信息与前台/后台选择条件 |
| child 的 task 描述 | `agent/src/builder.rs:668` 的分支使用 `CHILD_TASK_DESCRIPTION`，定义在约 932 行 | 保留有界、独立且有收益才递归委派的限制；不复制父级完整说明 |
| task output 描述 | `common/tool-types/src/task.rs:920`，`build_task_output_description` | 保留参数、超时、wait-all，只增加何时使用批量等待的短提示 |
| 当前任务事实 | task prompt、工具结果及现有 Timeline 上下文 | 目标、依赖、负责人、产物与验收情况放在这里，不注入动态 System head |

本表中 `agent/` 路径相对 `crates/codegen/`，`common/` 路径相对 `crates/`。完整路径在下文列出。

`crates/codegen/agent/src/prompt/template.rs:6-13` 通过 `include_str!` 嵌入 Markdown。只在仓库写一份新文案不会热更新既有二进制；后续实际发布仍需要正常构建。已有会话的稳定 System head 不应因文案更新被改写。

## Primary audience

目标文件：`crates/codegen/agent/prompts/audience/primary.md`。

| 当前位置/原文 | 保留的语义 | 具体修改与原因 |
| --- | --- | --- |
| 第 2 行：`You own the response to the user...` | 用户结果、任务级理解和整合责任属于主 Agent | 扩展为经验证的整体交付，明确优化整体耗时受正确性、用户约束和预算限制 |
| 第 4 行：`not to hand off the problem as a whole` | 不能把整个问题和最终判断一并外包 | 在新正文保留；复杂但局部的问题可以委派，跨模块取舍仍由主 Agent 负责 |
| 第 4 行：`Inspect the central evidence yourself before delegating` | 主 Agent 必须亲自理解影响全局的证据 | 改为关键判断需要核对证据，但已能界定的独立只读探索可以同时开始，避免将全部调研变成串行前置阶段 |
| 第 6 行：`broad fan-out is appropriate...` | 多来源、多假设确实可以扩大覆盖 | 不再单独鼓励广泛派发；改用独立就绪工作、传递成本和整合能力三个条件，避免为了占满槽位拆碎任务 |
| 第 6 行：`While delegated work runs...` | 主从可以重叠推进 | 增加不重复已委派内容、只做当前目标内有价值工作、自然检查点处理返回结果 |
| 第 6 行：`Wait only when...` | 无独立有价值工作时才等待 | 增加部分结果先验收，不按清单序号或整批完成顺序推进；实际等待语义仍来自工具说明 |
| 第 9 行：工具速记 | 询问与干预的用途区分 | 原样保留；新 audience 只引用通用的“available coordination/waiting mechanism”，不扩大具体工具声明 |

现有正文尚未明确的内容：依赖应指向最小充分输入；探索、实现、验收采用分别有条件；主 Agent 也遵守写入所有权；前提失效后只停止受影响工作；待验收成果积压时减少派发。这些都进入同一个 audience 正文，不增加独立 planner/verifier role。

## Subagent audience

目标文件：`crates/codegen/agent/prompts/audience/subagent.md`。

| 当前内容 | 具体处理 |
| --- | --- |
| 第 2 行有界任务、不能接管 parent session | 保留，并写明在范围内自行选择调查、实现和验证方法 |
| `supporting evidence and paths`、假设与集成影响 | 扩成有用的返回信息：完成程度、产物位置、关键输入、已执行验证、未验证部分、待决问题 |
| `instead of silently expanding the assignment` | 保留，并区分局部方法决策与共享接口、架构、范围决策 |
| `capability_authority` 整段 | 保持当前工作树版本，禁止用草案覆盖掉已存在的 approval-required、hard eligibility、exact-call Gate 和 MCP schema 说明 |
| 尾部 `ask_parent` 与权限提醒 | 原样保留；在新职责正文补充询问不启动父前台工作，受阻时可返回部分成果 |

不新增 `complete/partial/blocked/failed` 运行时枚举。完成程度使用自然语言表达；运行时 terminal outcome 继续由现有事实决定。特别是“子任务部分受阻”不等于把父 Goal 标为 Blocked。

一处需要细化的协作边界：现有 capability 段要求 hard-ineligible 操作通过 `ask_parent` 说明身份和原因，但 inquiry 不会启动父前台处理。新职责正文应说明：澄清足以解除问题时继续；若仍需要父 Agent 实际执行动作，则结束本阶段并在最终结果返回可定位的阻塞，不能一直等待 Sideband 替自己执行工作。

## 内置 role 与共享 foundation

| 文件 | 当前行为 | 方案 |
| --- | --- | --- |
| `crates/codegen/agent/prompts/agents/general-purpose.md:14` | 重复有界职责、证据返回和越界报告，同时要求端到端执行 | 保留端到端调查/实现/验证，增加核对调用方和实际周边代码；通用返回与范围协议由 audience 承担 |
| `crates/codegen/agent/prompts/agents/explore.md:17` | 只读、限定切片、区分事实与推断、返回路径 | 保留全部边界；增加明确问题、证据充分后停止、负面发现仅对实际检查范围成立，避免泛化为“全仓不存在” |
| `crates/codegen/agent/prompts/agents/grow.md` | 通用工作范围 | 无需添加另一份调度协议；primary audience 已覆盖 |
| `crates/codegen/agent/prompts/agents/grow-build-concise.md` | 简洁输出但保留正确性、验证和关键限制 | 保留；用场景评估确认简洁不会删掉依赖与验收证据 |
| `crates/codegen/agent/prompts/foundation/standard.md` | 通用调查/范围/验证和按能力渲染的工具指导 | 不加入主 Agent 专属职责；否则 child 也会收到，Full 角色又会缺失 |
| `crates/codegen/agent/prompts/foundation/mandatory-core.md` | 安全和基础调用规则 | 不修改；并行优化不能隐含扩大操作授权 |
| `crates/codegen/agent/prompts/extensions/session.md` | 条件化 memory 指导 | 不修改；任务调度不属于 memory 能力说明 |

所有 role 的 YAML frontmatter 保持原样，包括 `subagentOnly`、tool preset、`capabilityMode`、`inheritSkills` 和子代策略。文案调整不能改变工具权限或角色可见性。

## 工具说明：补实际使用条件，不改 schema

1. `TaskToolInput::run_in_background`（`crates/common/tool-types/src/task.rs:31`）实际默认是 `true`，对应测试为 `task_tool_input_defaults_background_true`。`task/mod.rs` 中“Blocking mode (default)”旧注释不能作为默认值证据。本方案保留实际默认值。
2. `build_task_description` 当前主要说明参数、Agent roster、resume 和 worktree，缺少完整的委派输入要求。在 Usage notes 增加目标、必要输入、产物、写入边界和验收的要求，不增加字段，也不要求固定 JSON。
3. 当前 description 声称子 Agent 接收“compacted version of project instructions”。`PromptContext::agents_md_user_reminder` 和 `child_prompt_delivers_full_agents_md` 显示该表述至少不适用于当前 Grow 路径。将这句替换为不假定所有 host 传输形态的指导：运行时提供项目规则与能力上下文，委派补充任务特定约束和证据。这里修正文案，不改规则传播或重建上下文系统。
4. `build_task_output_description` 已明确多 ID 加正超时为 wait-all；实现 `grow_build/task_output/mod.rs:305` 调用 `wait_all_event_driven`。补充“只有下一步需要全部结果时使用整组等待”，不能写成 wait-any 或承诺任意结果会中断这次等待。
5. `ask_parent` / `ask_subagent` 的工具定义已经写明 frozen context、tool-free Sideband 和不启动目标前台；`send_subagent_message` 已写明父到直接子、queued/immediate、durable receipt 不等于完成。保持这些定义，不复制其实现到新 prompt。
6. `resume_from` 是已有能力，续接时应重新检查相关输入前提。正在进行的取消生命周期修复独立负责可恢复终态范围，本 change 不承诺取消源一定可恢复，也不改 resolver。

## 不纳入的相邻清理

`crates/codegen/agent/src/prompt/subagent_prompts.rs:25` re-export 了 common `GENERAL_PURPOSE_PROMPT` / `EXPLORE_PROMPT`。后者有 `Maximize parallel tool calls for speed`，前者有“默认广搜”和详细 writeup 指导；但生产 shell child 通过 Agent definition discovery 取得 Markdown role，不能将常量直接当作当前生产子 Agent 正文。

本 change 不删除或统一这些 alternate/legacy 常量，也不以它们的测试代替真实 Markdown role 的装配验证。若后续实现发现活跃调用方确实依赖这些常量，应先记录实际调用链，再独立决定是否需要扩展范围。既有结构盘点可参照 `openspec/changes/inventory-all-crate-features/reviews/agent.md`；本次不启动遗留 prompt 系统清理。

## 测试分析

- `template.rs::primary_audience_retains_task_wide_synthesis` 与 `subagent_audience_returns_evidence_without_expanding_scope` 只断言现有关键词。换正文后应更新为新的职责锚点，并核对完整渲染结果，不能仅删掉失败断言。
- `context.rs::agent_role_is_separate_from_the_stable_system_head` 与 `full_role_composition_does_not_mutate_the_stable_head` 是装配边界回归，应保持。
- `builder.rs::child_task_description_is_concise` 限制递归委派说明小于 700 字节；本方案不加长它。完整委派契约从子 audience 与当前任务描述得到。
- task description 的名字替换、Agent 覆盖、空模型目录、参数名渲染等测试应继续通过；工具指导不能硬编码新的 namespace。
- 16 KiB 现有 prompt size guard 不是文案长度目标；未来记录实际拼接后的 head、role 和工具说明字节/模型 token 差值。
- 单元测试证明指引被正确投递，无法证明模型实际减少等待。行为实验和证据判定见 [validation-plan.md](validation-plan.md)。
