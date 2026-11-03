# Dashboard 与新版会话 UX 对齐分析

## 结论与范围

建议把 Dashboard 收敛为会话总览和管理入口；打开任何本地 root/fork 后，都进入同一套普通 AgentView 编辑体验。Dashboard 只提供来源、返回位置和当前目标标识，不再决定这个会话能否翻页、Esc 如何处理、输入发给谁。子 agent 是父会话内的下钻，fork 是独立的顶层会话；Agent selector 代表角色，不能与会话列表混为一个概念。

这是 UX 结构建议。前一轮的 runtime close、writer drain、canonical 删除保护、退出 scope、失败草稿保留都应继续保留。不能为了统一界面把这些事实检查退回由页面数量、颜色或本地状态决定。

## 依据和适用原则

本次读取当前工作区，而非只按旧截图推测。参照 `openspec/specs/client-surfaces/spec.md` 中 session command prefix、Dashboard stop、close、delete、loading navigation，以及 `docs/architecture/input-routing.md` 的输入/草稿所有权。当前主规范明确保留列表 Ctrl+X 的独立行为；如果采纳下文统一方案，必须有意修改该契约，不能称为纯重构。

已运行技能脚本：

- `--design-system`：developer terminal multi-agent 与 operations/data-dense dashboard 两组查询。
- `--domain ux`：focus、keyboard navigation、confirmation、error recovery、active state。
- `--stack html-tailwind`：仅参考焦点可见、图标动作有名称等通用交互原则。

第一组自动推荐误配为活动落地页；第二组 style 命中 Data-Dense Dashboard，但 pattern 仍含营销页 CTA。均不作为产品布局结论。Grow 是 Rust/ratatui TUI，不套用网页 hero、CSS、Google Fonts、移动端像素断点、SVG 图标或动效时长，不生成第二套 `design-system/MASTER.md` 权威。

可采纳的原则是：目标明确、焦点可见且移动可预测、相同动作标识一致、破坏性动作显式确认、等待和失败有就地反馈。W3C APG 的 [键盘界面实践](https://www.w3.org/WAI/ARIA/apg/practices/keyboard-interface/)区分焦点与选择并强调可预测性；[一致标识说明](https://www.w3.org/WAI/WCAG22/Understanding/consistent-identification)强调重复功能可被一致识别。这里借用交互原则，不声称 TUI 已达到网页无障碍合规。

## 已确认的冲突

| 冲突 | 当前代码事实 | 用户后果与判断 |
| --- | --- | --- |
| 同一会话因入口不同而换编辑规则 | `app/root/mod.rs` 的 `prompt_paging = !overlay_active && !minimal`；测试 `prompt_paging_scope_matches_agent_surface` 明确固定普通 inline/fullscreen 开启、Dashboard overlay/popup 关闭 | 进入 Dashboard 后，已学会的输入区翻页不再一致。属于已经存在的有意差异，不是上一轮修复遗漏的按键 bug。 |
| 三种会话容器并存 | 普通 `ActiveView::Agent`、携带 `attached_agent` 的全屏 Agent、`AgentDashboard` 内 attached popup 有独立路由；`DashboardReturn` 还区分 Agent/Overlay | Esc、左右键、退出与返回需按容器特判；前一轮 Ctrl+X 冲突正发生在这层。 |
| 选择会改变输入对象 | `views/dashboard/render.rs` 按 selected 自动构造 peek；`state.rs` 的 peek 在 dispatch 前消费文字，reply 与新建输入切换，Tab 又分别管理 list/dispatch 和 peek focused | 用户看似只移动列表，实际上切换了消息收件人和 Enter 语义；不同编辑子模式不易察觉。 |
| 搜索复用并清空派发草稿 | `DashboardState::enter_search_mode/exit_search_mode` 都调用 `dispatch.set_text("")`，没有保存原 dispatch 文本 | 从准备新任务切到搜索会失去当前输入。是明确源码行为，不能以“共用 PromptWidget”推断草稿一致。 |
| 切换预览清除回复及 undo | render 中 `apply_fields` 检测目标变化后调用 `clear_peek_reply`；该函数同时清空文字和编辑历史，注释说明用于避免跨目标错发 | 原方案用清稿避免错发；目标仍不稳定，且与新版会话按身份保留草稿的预期冲突。修复方向是固定目标/复用 composer，不是简单去掉清稿。 |
| 相近键/图标跨越不同后果 | 列表 Ctrl+X 停止/两次删除；attached 详情首按配置、再按停止或关闭；返回 frame 与删除行都存在叉号历史语义 | 之前已修到符合现有契约，但用户仍须根据位置、busy/idle 和时间窗口判断后果。功能可用并未消除设计负担。 |
| 状态与所在位置混在一条轴 | `RowState::Inactive` 是当前 Pager 未加载的 roster idle/dormant；本地 idle 是 Idle，磁盘 fallback 也构造 Dormant | Inactive 不能让用户理解为已终止、没有后台工作、可无条件删除；来源和执行状态须分开。 |
| 详情切换依赖总览过滤/实时排序 | `overlay_cycle_order` 复用 Dashboard 的 filter/grouping/pin/reorder；dispatch 在当前会话不在 order 时 no-op；排序还使用状态和 last_change_at | 搜索条件会使详情里的上一/下一会话失效，状态变化会改变邻居。ID 保住选中项不等于空间位置和切换顺序稳定。 |
| 总览复制完整会话功能 | Dashboard 自己实现回复、问答选项、权限拒绝文本、模型/Behavior staging 与 slash command context；peek 的 Up/Down 在选项边缘还会切换 agent | 新版会话每加一种交互，都要再维护一套总览版本。重复交互面才是长期冲突来源，仅抽公共编辑组件不够。 |

路径均相对于 `crates/codegen/pager/src/`。代码变化后的行号应以源码为准。Atlas scoped search 定位到 `DashboardState::handle_input`；其余事实核对实际文件和测试。列表重排、双高亮的实际误操作频率、不同终端的视觉对比度，本轮没有做用户测试或测量，不伪造发生率/可用性评分。

## 建议的页面与焦点模型

保留两种主场景：

1. **会话总览**：找会话、看活动、处理入口、新建、管理。默认列表焦点；预览默认只读，移动选择不发送、不改变编辑对象、不取消工作。新建可快速进入，但必须是清楚命名的“新会话”输入态，不与选中行回复共用隐式收件人。
2. **会话详情**：复用普通 AgentView、composer、队列、权限/Question/Plan 交互、选择器和翻页；入口来源只影响 breadcrumb/返回目标。保留模式能力的合理差异，例如 minimal 没有交互 scrollback；不把 Dashboard 来源当作另一种 screen mode。

子 agent 从父详情下钻，显示 `父会话 > 子任务`；fork 在总览中与来源平级，元信息标明 fork 来源。进入/返回不自动改 source、cwd 或用户输入，不把子任务停止动作提升成父级停止。

“快速回复”若是高频需求，可保留显式动作，但必须显示并锁定 session/interaction 目标，复用该会话的 composer 或既有待处理交互。列表继续移动不能转移收件人；返回时同一会话草稿仍在。第一步可先统一详情体验，随后再决定是否保留就地回复，不能为了界面简洁直接删掉并发管理效率。

以下为信息层级示意，非新增国际化要求：

```text
会话总览 · solaris-agent                  [新建会话] [搜索] [会话动作]
待处理 1   运行 2                         范围：本窗口 / 其他终端 / 已保存

> SDK Audit         等待确认   本窗口 · main · 3 个子任务
  Gall's Law        运行中     本窗口 · fork 自 SDK Audit
  PPT 架构          空闲       本窗口 · main
  Count deps        未打开     已保存 · 最后记录 1 分钟前

预览 · SDK Audit（只读）
待确认：执行命令 …                                    [打开处理]

↑↓ 选择   Enter 打开   Ctrl+\ 返回   ? 帮助         焦点：会话列表
```

“本窗口/其他终端/已保存”是来源，不替代活动状态。后端只提供 dormant 时显示“未打开”；只有权威事实才显示“已挂起/已结束”。不根据页面未加载推断是否有 writer。

## 动作与快捷键建议

| 意图 | 建议名称/入口 | 必须保留的边界 |
| --- | --- | --- |
| 回总览/原会话 | `Ctrl+\`、显式返回文字 | 纯导航，不停止、不清空草稿；返回恢复合理焦点和滚动位置。 |
| 配置当前目标 | `Ctrl+X` 配置前缀；保留 M/A/E/P/B | 总览显示被选中的会话，详情显示当前会话，child 保留 child 范围；无目标时禁用/说明，不能悄悄变成“下次新建的默认值”。新建配置另标“新会话配置”。 |
| 快速停止 | 若保留双 Ctrl+X，建议只做停止 | 空闲时绝不升级为关闭或删除；显示确切停止目标。停止当前 turn 与停止整个会话工作不能用模糊文案互换。原 Ctrl+C 沿用新版现有取消策略，不再叠加 Dashboard 策略。 |
| 关闭会话 | 命令/动作菜单中显式的“关闭会话” | 结束运行实例并保留历史，与“返回列表”区分；runtime ACK 前保留原绑定/状态，失败可重试。原运行不可恢复时不能承诺“继续原执行”。 |
| 删除历史 | 独立的“删除历史…”及持久可读确认 | 确认包含标题/目标身份及永久后果，不复用 2 秒停止前缀。不隐式级联；canonical 结果依赖仍会阻止删除。 |
| 退出进程 | 沿用既有挂起退出流程 | 只处理当前连接拥有的 runtime 范围；未确认时保留重试/Force，不把回总览称为退出。 |
| Esc/Left/Tab/PageUp/PageDown | 详情完全委托新版 AgentView | 去掉“因为来自 Dashboard 就额外返回/吞键”的规则。Esc 是否取消运行仍由现有会话策略决定，不能笼统改成所有地方 Esc 都无副作用。 |
| 上一/下一会话 | 保留既有 Ctrl+[ / Ctrl+]，基于稳定的打开会话顺序 | Dashboard 搜索只过滤总览，不缩小详情导航集合；状态更新不换邻居，关闭一项后按稳定邻居落点。 |

不预先发明整套新键位。动作菜单和帮助从已有 ActionRegistry/能力投影生成，列出当前目标、可用原因与真实按键；同名动作不维护第二份手写键位说明。新的默认绑定在实施 change 中核对冲突后确定。自定义绑定同样进入语义矩阵。

## UI 呈现

- 保留既有 Theme 和终端等宽字体；统一边距、行密度、状态词，不引入新的配色/图标系统。普通正文、辅助元信息、错误应有清楚层级；暗色和亮色分别检查，不从截图猜对比度达标。
- 键盘焦点只有一个强指示；鼠标 hover 使用弱提示，正在查看的会话用“当前”标记，不与列表选择使用同样的大色块。状态文字/符号与颜色同时提供。
- 每行优先呈现任务标题、真实活动状态、会话来源；分支/目录/模型降到次要位置。第二行显示待处理原因或现有消息的简短纯文本预览，不追加 LLM 摘要请求，不泄漏原始 Markdown 标记。
- 打开的会话采用稳定顺序，默认不要按每次活动重新洗牌；“待处理/运行中”做计数与可选过滤。若继续提供状态分组，明确它是总览视图，不影响详情切换；有编辑/菜单确认时不让目标随重排改变。
- 以终端列/行设计布局，不套网页像素断点。先保证约 80×24 能看见目标、主动作、焦点与输入；窄/矮时收起辅助预览，保留同一语义和快捷键。宽屏预览可扩展，不增加第二个默认编辑器。
- 等待反馈属于原目标：停止中、关闭中、删除失败及可重试说明就地展示。不能下一次任意按键就丢掉唯一错误说明，也不能把 transport 断开显示成成功。

## 关键用户旅程

**并行管理**：从 A 开总览，定位 B，Enter 进入与普通 B 相同的详情；输入和翻页无需重新学习。返回总览恢复 B 的选择；切回 A 保留其草稿、光标和 scrollback。状态变化只更新状态，不把焦点转交给 C。

**新建/fork**：新建明确进入新会话输入态；fork 后提示来源与新身份。成功才显示完成；失败保留未发送内容，空临时页回来源，不删除可能已创建的副本。新建与 fork 均使用同一详情壳，不新建另一种输入模式。

**处理待确认**：总览告诉用户哪个 root/child 的哪种交互待处理，打开该真实 Permission/Question/Plan 视图；回答仅绑定原 interaction。取消查看不代替拒绝或回答，也不把父/子目标混在一起。

**结束与清理**：返回列表 → 可继续查看原会话；停止工作 → 等待权威状态；关闭会话 → ACK 后释放本地/运行实例并保留历史；删除历史 → 单独确认并等待结果。删除因 canonical 链被拒绝时显示原因和真实关联，不建议通过直接删除文件绕过。

## 实施顺序与验收

| 顺序 | 实施范围 | 完成依据 |
| --- | --- | --- |
| 1 | 普通/attached 详情的编辑、翻页和 Esc 路由一致；统一输入 owner | 同一 session 从直接打开、Dashboard、fork、cycle 进入时，除返回元信息外行为相同；现有显式差异测试有意更新。 |
| 2 | 总览选择/搜索与草稿分开；peek 默认只读，待处理跳转到真实视图 | 搜索不清新建稿；换行不丢回复/undo、不换收件人；权限/Question/Plan 不复制第二套交互。 |
| 3 | 前缀/停止/关闭/删除动作分离 | idle/busy/loading/child/remote/失败下同一动作后果可预测；双击或重复键不跨越破坏性边界。 |
| 4 | 状态轴、稳定顺序和视觉层级 | 活动更新/搜索不影响详情循环；键盘焦点、hover 和当前会话可区分；窄屏和 CJK 宽字符不遮挡目标。 |

每步先建自己的 delta 并验证，不先建全局导航框架，也不先重排全部代码目录。优先复用 AgentView、ActionRegistry、typed interaction 和既有 retirement/draft owner；证明旧 popup/peek 路径无调用方后再删除，不凭“旧”字保留第二套长期兼容行为。

验收矩阵至少覆盖：inline/fullscreen、vim/non-vim、普通/总览进入/child、idle/busy/loading；编辑与搜索保稿；list focus 与 recipient 区分；菜单打开中状态变化；当前项被总览过滤；最后一页关闭；晚到失败不抢焦点；raw Control/kitty CSI-u 与自定义键位；80×24/矮屏/CJK/鼠标与键盘混用。用 AppView 真实输入入口加 PTY 组合路径，不能只测编辑组件或只断言 dispatch effect 非空。可用性结论还需实际任务观察，本轮不声称已完成用户测试。
