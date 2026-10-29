## ADDED Requirements

### Requirement: Continuous tool and Thought activity forms a stable group

终端 SHALL 将连续工具调用与 Thought 按原始时序呈现为可折叠活动组。组边界 SHALL 由对话、Skill 或独立交互记录决定；附着在工具成员上的待响应或 Hook 状态只改变可见性；工具类型、运行或终态、成员展开状态 SHALL NOT 拆分该组。Skill SHALL 保留独立行，并隔开其前后的活动组。纯 Thought 段 SHALL NOT 伪造成工具组；单次工具调用 MAY 保留现有单项动作呈现。真实工具类型 SHALL 来自工具的结构化身份，而非描述推断。失败工具 SHALL 保留在组内并可检查。

#### Scenario: Interleaved tools and Thought remain together
- **WHEN** 一段连续活动包含 Thought、不同类型的工具调用及其状态更新
- **THEN** 它们属于同一活动组，按原始事件时序展示；工具种类变化和运行转终态均不拆组或重排成员。

#### Scenario: Leading or trailing Thought retains the activity identity
- **WHEN** 工具活动以 Thought 开始或结束，且 Thought 正在流式更新、完成或因 `show_thinking_blocks` 被隐藏
- **THEN** Thought 仍保留在原始时序位置，组首 `EntryId` 与展开状态保持稳定；隐藏 Thought 正文不隐藏组摘要及其展开入口。

#### Scenario: Conversation, Skill, or standalone interaction separates activity
- **WHEN** 连续活动之间出现新的对话、独立 Skill 行或独立交互边界
- **THEN** 边界两侧的工具与 Thought 不合并，Skill 仍以独立行及既有身份和结果呈现。

#### Scenario: Thought-only activity has no tool group
- **WHEN** 连续活动只有 Thought 成员而没有工具调用
- **THEN** 按 Thought 的既有显示语义呈现，不显示虚构的工具组或工具数量。

#### Scenario: Group summary counts the whole group
- **WHEN** 活动组折叠，且组成员有一部分被折叠规则隐藏
- **THEN** 摘要仍覆盖整组成员；工具数量只计算真实工具调用，Thought 不计为工具。

#### Scenario: Failed tool remains inspectable
- **WHEN** 活动组内的工具调用失败
- **THEN** 失败状态和可检查内容仍属于原组，不被摘要、折叠或分组更新丢弃。

### Requirement: Activity group folding preserves active and manually revealed content

在 `show_thinking_blocks` 允许显示的前提下，折叠的活动组 SHALL 隐藏已完成 Thought，并保留活跃 Thought/工具预览、用户手动展开的成员、待用户响应内容及 Hook 信息。展开活动组 SHALL 按原始时序显示全部成员，并允许查看单项正文。`show_thinking_blocks` SHALL 保持其既有 Thought 显示语义；组聚合 SHALL NOT 使 Thought 绕过该语义。活动组经 dense truncation 后 SHALL NOT 再按组内成员被二次裁剪。

#### Scenario: Folded group retains active and explicitly revealed items
- **WHEN** 组处于折叠状态，组内存在活跃成员、用户手动展开的成员，或待响应/Hook 信息
- **THEN** 对应预览、显式展开成员和交互信息仍可见，已完成且未显式展开的 Thought 正文按折叠规则隐藏。

#### Scenario: Expanded group exposes ordered members and individual bodies
- **WHEN** 用户展开一个活动组并打开某一成员正文
- **THEN** 所有成员按原始事件顺序呈现，且该成员正文可查看，不改变其他成员的顺序或身份。

#### Scenario: Thinking blocks remain disabled or enabled as configured
- **WHEN** `show_thinking_blocks` 禁用或启用 Thought 显示
- **THEN** 活动分组沿用相同隐藏/显示语义；启用时展开组可查看完整 Thought 内容，切换 Thought 可见性不清除活动组的手动展开状态。

#### Scenario: Dense layout truncation encounters a grouped activity
- **WHEN** dense layout 对包含多个工具与 Thought 的活动组应用裁剪
- **THEN** 活动组不会在聚合后再被按成员二次裁剪，组摘要和适用的活跃预览仍可见。

### Requirement: Live activity updates and replay preserve grouping and user expansion

live 与 replay SHALL 基于相同事件序列得出一致的活动分组、原始顺序和组摘要。启用活动聚合时，手动展开工具或 Thought SHALL 不依赖另行开启 `respect_manual_folds`。流式追加、成员完成、工具类型细化及布局 resize SHALL 稳定保留组和成员的手动展开状态，不得因更新而重排成员、拆分组或收起用户已展开的内容。

#### Scenario: Live append and completion retain an expanded group
- **WHEN** 用户展开活动组后有 Thought/工具事件追加或成员从运行变为终态
- **THEN** 新成员按原序加入，原组和用户展开状态保持不变。

#### Scenario: Tool identity is refined after its description arrives
- **WHEN** 工具成员的结构化类型在后续更新中细化
- **THEN** 只更新该成员的真实工具身份，不改变组边界、工具调用身份、顺序或展开状态。

#### Scenario: Layout resize and replay agree with live presentation
- **WHEN** 活动组发生布局 resize，或同一事件序列通过 replay 呈现
- **THEN** resize 不改变分组和手动展开状态；replay 得到与 live 一致的成员顺序、分组和摘要。
