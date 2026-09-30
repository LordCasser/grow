# 架构证据与对抗性验证计划

调查日期：2026-09-29。下文记录实施前的证据和对抗性清单；完成后的逐项验收见 [verification.md](verification.md)。保留代码的历史状态见 [handoff.md](handoff.md)。

## 1. 已核对的证据

代码路径均相对 `crates/codegen/`，行号仅是调查时定位，实施以符号为准。

| 发现 | 证据 | 对方案的约束 |
| --- | --- | --- |
| 旧 replay 只有正文、两行 footer、滚轮/折叠；当前草稿改为 tree runner | `pager/src/replay_cmd.rs` 与 `replay_cmd/tree.rs::run` | 原缺口不能靠启用 mouse capture 修好 |
| 正常布局包含真实 composer、queue、turn status、CTA、followups | `pager/src/views/agent.rs::AgentViewLayout::compute`，`app/agent_view/render.rs` | 新 panel 不调用 PromptWidget 或正常 action registry |
| 子视图隐藏输入框不消除 live action | `app/agent_view/session.rs::mark_as_subagent_view`，`app/agent_view/render.rs` | 继续保持独立离线入口 |
| HintItem/ShortcutsBar 是纯提示组件，支持 compact；不提供鼠标命中 | `views/shortcuts_bar.rs` | 复用提示绘制，控件另记真实 rect |
| selection_box 是 render 产物；不绘制就看不见键盘选择 | `scrollback/scrollback_pane.rs`、当前 `replay_cmd/browser.rs::draw` | 正文渲染需消费完整被动交互输出 |
| 展开 verb group 的首成员与 header 共享 entry | `scrollback/state/`、`app/agent_view/selection.rs` | 用实际行区分，不把 header 当 member 0 |
| BlockViewer 大部分工厂纯读取；BgTask 正常路径读 runtime store | `views/block_viewer.rs`、`app/root/dispatch/transcript.rs` | 共享纯 helper，缺历史输出时提示，不接入 live store |
| 正文拖选不是 ScrollbackPane 自动提供 | `scrollback/text_selection.rs`、`app/agent_view/selection.rs` | 草稿 viewer 支持拖选不等于正文已具备 |
| 搜索缓存按 content_generation 全量重建，建议 open/query 时同步 | `scrollback/search.rs::ScrollbackSearchIndex::sync` | 不在每帧/每个隐藏节点创建全文副本 |
| 树快照已固定 updates 句柄/长度，校验 seed 与 owner | `shell/src/session/storage/transcript.rs::capture_tree_at/verify_seed` | 复用同一快照；不能点击时 recapture；并非全树同一瞬间原子快照 |
| seed.parent_spawn_seq 指 owner Timeline；owner 不一定是 UI 直接父节点 | 同上、`chat-state/src/timeline.rs::SubagentSeedEvent` | 锚点与可导航父子关系分别保存 |
| 工具开始事件早于 prepare/permission | `shell/src/session/actor/tool/mod.rs:160,223`、`tool/preparation.rs:791,845` | 普通 tool span 不可准确分离 permission 等待 |
| PendingInteraction/InteractionResolved never persisted | `shell/src/extensions/notification.rs:1171` | 不可承诺还原所有 NeedsInput 开始/结束 |
| Plan approval 有控制状态，但不能代替其他并行活动 | `shell/src/session/actor/tool/mod.rs` 的 approval_pending 处理，`session/control` | 匹配 owner/revision，扣除执行保护并集 |
| 冷恢复时间是 reopen 时刻，会合成 unknown/cancelled duration | `chat-state/src/timeline.rs::recover_interrupted` | 中断空档要单独标估算，不能把全 duration 当执行或 IDLE |
| TaskCompleted Grow 可移除完整 output | `shell/src/tools/notification_bridge.rs:483`及现有 session-timeline 契约 | 不从当前任务文件重新获取“历史”输出 |
| Goal、Behavior、Turn、Workflow 归属彼此独立 | 主规范 `behavior-goal`；归档 `harden-offline-behavior-goal-projections` | 不以 Goal paused 或 turn cancelled 替代 IDLE 分类 |

## 2. 本轮保留草稿暴露的盲区

1. `transcript.rs` 已引用尚不存在的 `activity` 模块，是已知构建阻塞；草稿不是可交付实现。
2. `tree.rs` 当前仅通过 parent display Spawned 搜索估算锚点，尚未接入 owner Timeline 锚点，也未实现 IDLE 分段映射。
3. 当前全树 round-robin 只有事件条数限制；隐藏节点 reveal、复制/搜索、超大单事件的成本仍需验证。global time 当前直接取目标时钟，有显示领先交付 frontier 的风险。
4. `Browser` 已接 detail/search，但正文文字拖选、播放中的旧命中失效、稳定 entry ID、普通方向键语义和 copy 失败反馈尚未闭合。
5. 当前 footer 仍是两条静态字符串，无响应式面板、控件 rect、help 摘要、IDLE 状态提示。
6. 旧单节点 Player 测试改成 test-only harness，不能证明生产 tree 协调器正确；必须直接测试生产路径，避免两套逻辑各自通过。
7. `time.rs` 只写了固定 offset / None 分支测试，未运行；本机解析失效、DST、混合缺失时间仍未验证。
8. 根/子节点存在被 rewind 隐藏的分支，不能让无法导航的历史拖长最终播放；时间保护与可见播放集合需分别定义。
9. 当前只考虑 primary Timeline 会遗漏 sideband/后台任务活动；准备期需在现有验证边界收集，不能另连 scheduler。

## 3. 验证矩阵

层次说明：U=确定性单元/纯函数；I=真实来源夹具与投影集成；B=Ratatui buffer/真实布局；P=PTY 输入与终端恢复。夹具使用合成历史，不把用户 session 正文保存到仓库。

| ID | 场景 | 必须观察的结果 | 层次 |
| --- | --- | --- | --- |
| V01 | 消息、thinking、execute/read/edit/search/communication/raw/data 详情 | 截至游标的正文可查；关闭回原位置；无未来正文 | U/B/P |
| V02 | 收起/展开 group，header 与第一成员，sticky prompt，空白点击 | 正确目标，文字拖选不触发 open | B/P |
| V03 | 跨行/跨条目/中文/组合字符拖选，选择后 streaming append | 正确复制内容；稳定锚点；失败有反馈 | U/B/P |
| V04 | 搜索输入 `q + /`、bracketed paste；输入退出多层 Esc | 不误退出/调速/执行；`]` 不泄漏、Esc/q 逐层退回，帮助层 q 不退出根，Ctrl-C 全局退出 | U/P |
| V05 | root→child→grandchild→返回，切页前后滚动/折叠 | 一条时钟；各自恢复位置；card 未交付前不可打开 | I/B/P |
| V06 | 子节点时间缺失/早于 spawn，owner != delegating parent | 正确因果门控，可靠锚点或明确估算，不因点击而重置 | I |
| V07 | sources 持续 append、子来源坏/缺失、seed 错误、预算超限 | 固定截点或启动前明确失败；不展示静默部分成功 | I/P |
| V08 | rewind 排除子分支、直接 child ID 开始 | 不泄露隐藏/兄弟正文；不可达节点不拖长进度 | I |
| V09 | 30s、30s+1ms、2h 已确认 idle，0.5×/1×/16× | 仅 >30s 压缩为 1s P；提示原长度；暂停连续 | U/I |
| V10 | root 无输出而 child/孙节点/sideband 请求仍执行 | 无默认 IDLE 压缩，切换视图不改变分类 | I |
| V11 | 正常长推理、工具无输出、工具权限等待边界未知 | 保守保护；不得把未知当已确认 IDLE；可显式下一记录 | I |
| V12 | 可靠 Plan 等待，与另一子请求/后台任务重叠 | 只压缩全树无执行覆盖部分；approval 本身不能操作 | I |
| V13 | 关闭后 3h 再开，恢复 request/tool/hook terminal | 中断空档标估算；长 duration 不伪装运行/成功 | I |
| V14 | pending 无终态、只有 session 打开无输入、最终离线无末事件 | 截点即结束，未知状态保留，不等到 now | U/I |
| V15 | idle 内多次 Goal 用量、Behavior 更新或 heartbeat | 按原顺序全部投影；不把连续空闲碎片化 | I |
| V16 | reveal→用户打断→新输入→旧工具 terminal 晚到 | 正文归属不串、取消立即可见、旧 terminal 更新原工具 | I |
| V17 | Goal set/pause/resume/clear/replace/退出模式，中途取消，late usage | footer 历史状态随当前投影；播放器不自动暂停/继续；不生成续轮 | I/B |
| V18 | Plan confirmation/rejected、Workflow 模式退出但 Run 未结束 | 不提前改模式，不结束 Run，无可批准控件，不推导全局 IDLE | I/B |
| V19 | DST 前后/重复小时、UTC+08、负偏移、resolver None/越界 | 原间隔相同，本地时间按记录时刻，回退/未知标签正确 | U/B |
| V20 | 极高倍速、所有节点同 due、长 Markdown、多节点 reveal | 每条恰好投影；输入持续处理；百分比无提前 100%；记录峰值内存 | U/I/P |
| V21 | Playing/Paused 下换速/打开子页/下一记录；finished 后查看 | 保持 Playing/Paused；显式按钮可在局部焦点下前进；完成后保持可浏览 | U/P |
| V22 | 120×40 到 40×8、零/单行尺寸、CJK 长 title、overlay 中 resize | 无越界/陈旧按钮；提示按优先级；正文保留可用空间 | B/P |
| V23 | 模态搜索 pending 结果在切页/关闭/新查询后到达 | 旧 generation 不抢焦点/滚动；只搜已显示内容 | U/I |
| V24 | 同一快照多倍速、暂停、下一记录、IDLE 压缩、遍历各 child | 各节点最终归一化内容/业务状态 = 对应 export；panel 提示不计入 | I |
| V25 | 含命令、审批、URL、workspace路径、任务文件、代码 fence 的历史 | 零执行 session/provider/工具/Hook/恢复调用；零 browser/opener/renderer 调用；复制仅可触发明确 clipboard backend；源文件哈希不变 | I/P |
| V26 | 正常 AgentView 复用新 viewer helper | 原详情/raw/data/copy/BgTask 调用行为不回归 | U/I |

## 4. 验证方法与限制

先用可注入时钟/offset/来源的 U/I 测试校验语义，再用真实 Scrollback 布局校验命中和截图 buffer。PTY 必须实际发鼠标 press/release、bracketed paste、resize、F8、层级 Esc，不能只调用 handler 伪装终端验收。调度测试对生产 tree 类型执行，不复写简化 Player。

只读证明结合结构审查（无 session runtime/transport 创建路径）、被调用入口的测试探针和临时 session/workspace 前后哈希；哈希相同本身不能证明没有网络或子进程，所以不能单凭哈希声称全隔离。clipboard 可由用户动作调用并单独替换为测试 backend，不把其实现细节误算成执行历史命令。

复用 `docs/development.md` 的定向命令：shell transcript、pager transcript_projection/replay_cmd/export_cmd、block_viewer 及正常调用方回归。实际新增 module 后测试 filter 必须涵盖生产 tree/browser/time/idle，不能只跑旧 `replay_cmd::tests`。CLI 编译和真实 PTY 仅在实现阶段执行。

本次为文档阶段，以上 V01–V26 尚未以本 change 的实现运行。旧 change 的测试通过记录不能迁移成这些新场景的通过证据。实现 agent 应把命令、通过/失败、未覆盖项和资源数据记在 change 内，然后再勾选 tasks 与归档。

## 5. 明确保留的限制

旧数据无法完整还原所有人工等待和精确关闭时刻；默认保护不确定执行、估算恢复空档、提供显式下一记录。若要获得准确历史 NeedsInput 和进程离线区间，需要单独的持久化契约，不在本 change 顺带重做 Timeline。

来源捕获有读取上界，但跨会话不承诺全局原子截点；慢磁盘的单次 OS read 不承诺硬截止时间。精确 token cadence、随机 seek/倒放、外部文件的过去版本、未记录后台输出均不在可恢复范围。

## 6. 文档复审闭环

Luna 只读复审指出并已在文档中修正：单行终端退出提示与正文保留冲突；根纯选区/帮助层的 q 归属；外链/opener/renderer 禁令必须进入 delta 和 V25；Playing 下显式前进与搜索 `]` 不冒泡的验收缺口。本轮只修订方案，未据此修改功能代码或运行实现测试。
