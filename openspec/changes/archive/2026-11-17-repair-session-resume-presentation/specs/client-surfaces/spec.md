## ADDED Requirements

### Requirement: Main transcript scrolls through bounded history windows

普通主 view SHALL 将按需历史页显示在同一有序 transcript 中；键盘滚动、滚轮、滚动条及 turn 导航接近未加载边界时 SHALL 触发相应有界页读取。窗口 SHALL 支持向旧和向新续读，保留稳定实体、选择和 viewport 逻辑位置。正文、工具及完成结果跨页 SHALL 合并为与同截点完整投影一致的内容，重复或重叠读取 SHALL NOT 复制内容。历史页 SHALL NOT 修改当前控制、live cursor 或用量账本。

#### Scenario: Three pages load through ordinary scrolling
- **WHEN** 用户 resume 至最近页并通过滚轮、PageUp 或滚动条访问更早历史
- **THEN** 相应内容加载进主 view，可连续上下浏览；后续页不替换成独立页面，也不丢失上一段浏览能力。

#### Scenario: Prepending while live updates arrive
- **WHEN** 用户停在多行消息中间且选中内容，旧页返回的同时到达实时事件
- **THEN** 原实体、选中内容和 viewport 逻辑行保持，历史及实时内容各出现一次。

#### Scenario: A response and Tool span page boundaries
- **WHEN** 长响应、Tool start/progress/terminal 以不同读取顺序或重叠重试跨页到达
- **THEN** 最终字节和可见顺序与同 cut 的完整投影一致，一个 Tool 保持唯一实体，旧 progress 不覆盖 terminal。

#### Scenario: A page fails or its owner is replaced
- **WHEN** 读页失败，或期间切换会话、重绑、切换分支或发起新的历史跳转
- **THEN** 失败保留可重试边界和既有内容；失效结果不覆盖新窗口、控制、选择或执行 readiness。

#### Scenario: Minimal initial recent page
- **WHEN** Minimal 恢复会话并首次成功采用最近页，期间可能已有 live 回声完成 native 提交
- **THEN** 最近页尚未打印的实体通过正常 native commit 输出一次，已打印的回声保留原提交事实；后续补页和重读不将已打印内容重新置为待输出。

#### Scenario: Minimal already printed recent rows
- **WHEN** Minimal 的最近内容已经提交到 native terminal，用户随后浏览更早历史
- **THEN** 历史通过可重绘的有序虚拟窗口呈现，已打印顺序不被伪造修改；返回 live 不重复提交旧页或已打印实时内容。

证据入口：`crates/codegen/pager/src/app/agent_view/input.rs`、`app/root/dispatch/task_result.rs`、`scrollback/state/`、Minimal 渲染 seam；`crates/codegen/shell/src/agent/mvp_agent/history_paging.rs`。

### Requirement: Timeline locates unloaded canonical history

开启右侧时间线时，客户端 SHALL 额外按需读取与正文同 session/branch/cut 的轻目录，包含每个可见用户消息发生的稳定位置与有限预览。目录范围 SHALL NOT 受正文已加载范围限制，重复文本 SHALL 保留不同发生身份。点击未加载目标 SHALL 直接读取包含该目标的有界邻近窗口并在主 transcript 显示，不以遍历所有中间正文页作为前提。

#### Scenario: Navigate to an old unloaded prompt
- **WHEN** 主 view 只加载最近页，用户点击右侧时间线较早位置
- **THEN** 客户端按该位置加载必要目录与目标附近正文，精确对准所选 prompt；之后可向两端续读或返回 live tail。

#### Scenario: Identical text occurs twice
- **WHEN** 两条不同历史用户消息文本相同
- **THEN** 时间线保留两个可区分目标，点击分别定位各自原始发生位置。

#### Scenario: A jump is superseded or unread history lies between windows
- **WHEN** 目标读取期间用户选择另一个目标，或远跳窗口与 live tail 不相邻
- **THEN** 旧结果不抢占新目标，未加载区间保持明确，不将不相邻历史拼成连续内容。

证据入口：`crates/codegen/pager/src/views/timeline.rs`、`app/agent_view/{render,mouse}.rs`、`scrollback/state/timeline.rs`；Shell history/projector 及 user echo/boundary 索引。

### Requirement: Token surfaces preserve accounting scope and completeness

客户端 SHALL 区分当前上下文占用、lifetime 消费和逻辑 Turn 结果用量。上下文占用 SHALL 来自当前请求投影，lifetime 与 Turn 结果 SHALL 来自可恢复的已结算账本；正文分页、折叠、窗口卸载与时间线跳转 SHALL NOT 重算或累加消费。各结果格式 SHALL 保留总消费完整性、缓存分类可用性与未知费用，不得将未知费用显示为精确零。

#### Scenario: History loading changes display but not spending
- **WHEN** 用户反复加载旧页、远跳、折叠或卸载窗口
- **THEN** lifetime token、模型分项及已有完成结果不变；新真实结算仍准确且只计一次。

#### Scenario: Context shrinks while lifetime grows
- **WHEN** compaction 缩小当前请求上下文，而主模型或 Sideband 产生新消费
- **THEN** 上下文占用可下降，lifetime 按实际结算增加，二者名称与来源明确，不互相覆盖。

#### Scenario: Child consumption is not finally settled
- **WHEN** 会话有尚未最终结算的模型调用或子 Agent
- **THEN** 当前累计以已记录消费呈现，并保留仍有待结算工作的可见语义；阶段性 child 快照不与最终累计 bill 重复相加，不把瞬时未结算伪造成精确零或永久未知事实。

#### Scenario: A result is incomplete in streaming messages output
- **WHEN** 本轮消费或费用不完整，最终结果经 streaming-messages-json 输出
- **THEN** 输出保留不完整语义，未知费用为 null 或缺失，不能只在内部日志告警后输出外观精确的零费用。

证据入口：`crates/codegen/pager/src/acp/meta.rs`、`headless/reducer/messages/{usage,wire}.rs`、TUI session usage reducer；`crates/codegen/shell/src/session/actor/updates.rs`。
