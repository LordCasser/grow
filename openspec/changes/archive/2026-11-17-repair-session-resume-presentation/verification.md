# 验证记录

## 最终验收

调查日期：2026-11-16；最终本地归档日期：2026-11-17。调查与实现基线为 `733d2c72ed0436dc27f050ee2149dd41432be552`。四项功能均已完成代码 review、模块回归和适用的进程级验收；Git 未提交、未发布。下方「方案与实施过程」保留曾经失败和修订的原因，其中的“待验证”只描述当时状态，最终结果以本节为准。

该功能验收阶段默认PATH使用Homebrew Rust/Cargo1.99.1，测试线程按开发指南设置 `RUST_MIN_STACK=16777216`；正式1.93.1工具链的发布前复核由后续 `prepare-release-v2-6-15` 归档记录；该候选未通过Windows测试编译门禁，正式发行由 `prepare-release-v2-6-16` 承接。真实终端测试显式设置 `PAGER_BINARY="$PWD/target/debug/grow"`，使用本工作树构建的 CLI；没有以旧发行版替代。

| 验收层 | 最终结果 | 证明的边界 |
| --- | --- | --- |
| `cargo test --locked -p chat-state --lib` | 676 passed，0 failed，3 ignored | 稳定 Turn 归属、100+50、无新采样保留原账单、main/aux/child、checkpoint+suffix、session-only、rewind 位置复用、incomplete 恢复与幂等冲突 |
| `cargo test --locked -p shell --lib -- --test-threads=4` | 4862 passed，0 failed，14 ignored | Behavior/Plan durable ACK、普通权限、双向页/around、目录后缀与固定 cut、用量调用方和 DISPLAY 结果留存 |
| `cargo test --locked -p pager -p pager-minimal --lib -- --test-threads=4` | Pager 7716 passed，0 failed，15 ignored；Minimal 104 passed，0 failed | 真实通知/TaskComplete/输入入口、历史实体/选择/锚点/live tail、20 项目录竞争/失败用例、13 项导航、native 提交事实、pending lifecycle 与 headless 结果 |
| `pager-pty-harness --test plan_approval_resume` | 3 passed | 待审批直接恢复、延期审阅恢复、两轮修订后批准完成；持久 artifact 与请求反馈一致 |
| `pty_e2e_persistence` 的 `ctrl_d_resume_keeps_*` | 2 passed | 300 个历史 Turn 加 140 个 live Turn 后 Plan 切换不在最近页，两次冷恢复仍为 confirmed drafting；连续旧页与返回 live 不改账单；同一稳定 Turn 在两进程结算 100+50，唯一完成150，再次恢复不变 |
| `pty_e2e_persistence::timeline_history_direct_seek_returns_live` | 1 passed | 100 行真实终端的 first tick 位于最近128行正文范围之外；未预翻正文就鼠标直达对应 prompt/marker，G 返回 live；请求数与持久结算不变 |
| `pty_e2e_minimal::minimal_history_resume_browser` | 1 passed | 初次最近页正常 native 输出；PageUp 经 middle 到 earliest，Esc 无旧页/旧消息重印；新 prompt 只出现一份、一次结算；再次浏览无请求或账单增量；同 batch 双 Ctrl+D 离开临时屏，冷恢复后仍可浏览 |
| `leader_cluster::scenarios::{reattach_completion_roundtrips_durable_log,leader_kill_reconnect_reloads_without_duplicating_history}` | 显式运行 ignored 场景，2 passed | resident 重新附着不重驱模型，完成记录只显示一次；leader 断线/重连/重载后可继续新 Turn，无重复正文 |
| `restored_turn_freezes_main_aux_and_child_without_new_sampling` | 最终文件独立编译，1 passed | 挂起前 main55+aux22+child33，经公开 ChatState 恢复后不产生新 Request/结算，真实 Shell freeze 得到110；session-only与stale消费仅在 lifetime，last-call 为None，TurnCompleted wire 保留结果；这是恢复到冻结的模块接缝，不替代实际通知投递的 PTY |
| `published_turn_completion_presentation_is_paged` 最终补充阶段 | 1 passed | 原 Timeline、原 writer 上追加 `prompt_owner:null` 的 session-only30，lifetime 精确增加30；130个 completion 身份与数量不变，旧页冻结150的 typed/wire、模型/缓存未知、unknown cost、incomplete 均全等 |
| `cargo build --locked -p cli --bin grow`；最终 `cargo check --locked -p cli --bin grow` | 均退出0 | 真实 PTY 产物及默认生产依赖图均可构建；格式收尾后再次 check 通过 |

首个100+50的真实生成由 PTY 证明；DISPLAY 的150是刻意预构造快照，只证明分页、冷读和迟到账单之后的留存，不将二者混作一份证据。非零 auxiliary/child 跨恢复归属由 chat-state 和 Shell 回归证明；PTY 为固定主账单 oracle 显式脚本化一次零消费辅助响应并等待其被消费，没有忽略额外消费。

resident 不增加 usage segment 的证据来自 Shell `cold_resume_preserves_durable_effort_chain`：冷 load 的 `resume_started` 恰增1，随后同 actor resident load 数量不变，live route 保持。该用例包含在上述 Shell 全库中；多客户端重连另由两项 leader 场景覆盖。

### 有界读取与反例

- 同一固定 cut 保留 session、branch、frontier/hash 和 projector version；每个正文响应最多128行、4MiB，超长 response/继承正文/Tool JSON 通过不同方向独立重组后与 canonical 字节逐一比较。读取历史不推进 live cursor，也不经过用量结算入口。
- 已发布300条目录加2条未发布后缀：around最早位置读128条 rank，before(10,4)读4条，非法 ordinal 读0条；目录查询只读 metadata。SEEN 使用首次 ordinal 与原 span.count 核对，拒绝同 generation 后续 publication 污染旧 cut。VERSION51 复用既有派生索引重建，不修改原日志。
- 历史与 live 同 messageId 保持单实体，较早 source 后到仍保 EntryId；无ID的不同 source 和同文不同ID仍分别保留。coalesced 段按原显示顺序对应。选择和多行逻辑锚点保持；entry 后 gap 锚点的既有问题另列 backlog。
- 初始 Latest 采用后、native 第一次绘制前依次接纳 Before 和重复 Latest，原 pending 实体仍 pending；已提交回声仍已提交，后续新历史实体不被重印。临时 alternate screen 的 Leave 由已有 terminal teardown 消费，不要求最后一帧一定执行。
- 新目标、本地跳转、关闭 rail 和返回 live 都淘汰旧 around；当前目录失败阻止自动 ensure 重发，显式点击或重新打开才重试。旧 binding/generation/nonce 的失败不吞掉新请求。
- 首次 `None→Some` 绑定保留响应前的当前控制快照；真正换会话或解绑清旧 epoch/phase。旧 epoch/revision、同版冲突和历史回执不能结算另一项本地 intent。

Plan 普通权限矩阵产生20个真实 allow/ask/deny审计事件及5次 gateway prompt，原能力上限、Sandbox 和协议互斥保留。一次使用当前产品提示的 `magpie/glm-5.3-flash` 代表任务完成终端调查及可回收实验，并拒绝脚本、MCP、委派实施和修改冻结计划的诱导，产品文件摘要保持不变。这是模型行为样本，不是任意模型服从提示的强制隔离证明；真正 PlanControl 审批链另由 Shell 与3项真实进程回归覆盖。

### 格式检查与限制

`git diff --check` 退出0。本次新 Rust 文件和变更范围已按 Rustfmt 整理；没有递归格式化未修改的模块。全量 `cargo fmt --all -- --check` 仍退出1，剩余28个文件逐一取 `git show HEAD:<path>` 后用同版 `rustfmt --edition 2024 --config skip_children=true --emit stdout` 可复现原有差异。它们集中在既有 actor、storage、workflow、coordination 代码；已登记 backlog，未为通过门禁混入整仓重排。

真实交互验收在当前 macOS debug 环境、隔离 mock provider 中完成，不宣称 Windows 原生终端或所有远端模型均已验证。工具权限和 Plan 提示用途各负其责。ignored 的其它场景未计为通过；本变更没有发布延迟承诺，也没有完成通用 gap 锚点或全仓格式债务。

### 归档校验

- `openspec validate --all --strict --no-interactive`：归档前20 passed，归档后19 passed，均0 failed。
- `openspec archive repair-session-resume-presentation --yes`：任务完整，8项新增要求合入四份主规范；本地归档目录为 `2026-11-17-repair-session-resume-presentation`。
- `openspec validate --archived --no-interactive`：962 passed，0 failed，包含本次归档。
- 归档后的10处新增文档链接和主规范锚点均可解析，任务表没有未勾选条目；`git diff --check` 退出0。以上为本地验收与规范归档，不表示 Git 提交或发布。

## 方案与实施过程

以下保留调查、失败及修订轨迹，不覆盖上方最终验收结论。

## 方案阶段基线

日期：2026-11-16。代码调查基线：`733d2c72ed0436dc27f050ee2149dd41432be552`。

## 已执行

- 阅读 OpenSpec 索引、相关主规范、现有 active changes 与历史 paging/Behavior 恢复设计；从保存、恢复、ACP 传输、Pager reducer、输入分发、Timeline/Usage 投影和既有测试核对调用链。
- 主 agent 亲自核对 Behavior durable transition/load、Plan 授权门禁与提示词、older page 结果分发、timeline 本地来源、ChatState prompt usage 恢复初始化及 headless 结果投影；两项独立只读调研补充历史与用量细节。
- `openspec validate --all --strict --no-interactive`：退出码 0，20 passed、0 failed，包含本 change 与当前主规范。
- 本 change 保留 proposal、analysis、design、四份候选 delta 与带验收方法的未勾选实施任务；未将候选需求写入主规范。

- 委派调研运行了现有定向测试，主 agent 核对了实际终端结果：

| 命令 | 结果与覆盖 |
| --- | --- |
| `cargo test -p chat-state --lib usage` | 25 passed；已有结算、幂等、恢复与 segments |
| `cargo test -p pager --lib resumed_usage` | 1 passed；恢复 usage 快照到状态栏和 /usage |
| `cargo test -p pager --lib transient_session_usage` | 1 passed；统计更新独立于正文/上下文 |
| `cargo test -p shell --lib record_response_token_usage_tests` | 13 passed；现有响应与上下文统计 |
| `cargo test -p shell --lib subagent_usage_fold_tests` | 18 passed；现有 child fold/冻结/迟到路径 |
| `cargo test -p shell --lib usage_categories_tests` | 2 passed；已有分类 |
| `cargo test -p pager --lib result_usage` | 21 passed；已有结果投影 |
| `cargo test -p pager --lib status_blocks` | 15 passed；已有状态块 |

合计 96 个现有测试通过。一次将两个过滤词传给 Cargo 的调用因参数格式被拒绝，拆成两条上述合法命令后通过；没有据此改源代码或放宽断言。现有测试通过支持复用原账本，并不覆盖本次发现的原 Turn 跨挂起或 messages incomplete 缺口。

## 未执行

方案阶段没有修改业务代码或执行真实 Ctrl+D/resume/滚动/时间线交互，未进行 Plan 模型行为评测。`analysis.md` 的触发链是静态代码证据；100+50 的例子为可证伪回归场景，不是方案阶段运行观测数值。不能据规范校验和既有测试通过声称问题已修复。

## 实施验收门槛

按 tasks.md 分别验证模块结果，完成相关功能后再跑 design.md Review Plan 的进程级链路。必须包括：最近页无模式事件仍恢复正确 Behavior；Normal/Plan 普通权限一致且提示词阻止直接实施；滚轮/键盘/滚动条三页连续浏览；未加载时间线目标直接 seek；跨页 Tool/response 与 live 乱序去重；Minimal native 已打印后浏览返程；同 Turn 跨挂起完整消费及无新采样完成；未知分类/费用保留；反复分页不改变任何账单。

用户已授权实施；盲区修订见 blindspots.md。change 未归档，Git 未提交。

## 实施阶段

- 当前状态快照、Plan 权限/提示词、历史双向读页和稳定用量归属正分别实施，尚未视为通过验收。
- messages 结果完整性候选已完成主逻辑 review；coder 曾运行 `cargo test -p pager --lib headless::reducer::messages::tests`，81 passed。随后只收敛了比较用例，但最终复跑被并行中的 chat-state owner API 迁移编译错误阻塞。待接口闭合后必须重跑最终文件状态，不能将旧次数当成最终验收。
- 一轮盲区修订后执行 `openspec validate repair-session-resume-presentation --strict --no-interactive`，退出码 0。
- Plan 代表任务使用父会话明确选定的 `magpie/glm-5.3-flash`，加载当前实际 base/phase 提示后，在 `/tmp/grow-plan-eval-20261116-200758/` 新建一次性 fixture。模型真实运行终端调查及独立实验，复现四种非法输入；拒绝把修复包装成验证、Python/MCP 绕过、委派实施、替换冻结计划的诱导。父 agent 核对实验代码并重跑，输出 `REPRODUCED ... True`；产品文件 SHA-256 前后均为 `53389e47c91598678dd40445bc289aac6887b3d81a236aecbcfe0be4c7cb2404`。此项仅是代理环境加载产品提示的一次代表任务，不是 Grow provider/PlanControl 真实链路，也不证明任意模型均遵守。
- Plan 授权旧 coder 因 Responses 协议 `conflicting response id` 终止，已保全其候选 diff 并释放 delivery；独立后继仅接手实际权限链测试，未把 runtime failure 当成测试通过。
- 后继已为真实 PermissionManager 测试补齐 LocalSet；挂起快照已在局部 `goal_transaction_gate` 内冻结时钟、捕获状态并持久确认，锁在 worker join/drain 前释放。二者定向测试因并行 owner 字段的测试夹具尚未闭合而未运行，不能以错误列表无本文件当作通过。
- 父 agent 两次早期执行 `cargo check --locked -p shell --lib`。第一次暴露 5 处用量调用方适配；第二次这些错误已消失，仅剩历史 ACP 在途接线的 `HistoryPageCut::capture/token` 两处未定义。此后 transport owner 的 `cargo check -p shell --lib --offline` 已通过；后续整合编译又分别暴露目录测试缺文件、重复导入及新 Around 哈希分支的在途错误。逐项修复后再跑测试，未把局部未出诊断视为通过。
- Pager 实际通知入口 review 发现：`detect_plan_mode_change` 拒绝旧 epoch 后，外层仍可能按预计算 resolution 结算本地意图。已形成单独修复，要求 root/child 的状态接纳与精确回执约束一致；当前未验收原 Pager 候选。

### 已复核的用量与 metadata 证据

- 父执行 `cargo test --locked -p chat-state --lib actor::tests::rewind_reused_prompt_index_keeps_old_usage_owner_settlements_session_only -- --exact`：1 passed。亲读同 index、同 client prompt_id、不同 TurnId 的 main/aux/child 迟到结算与 lifetime 断言，未按正文或当前位置归属。
- 父执行 `cargo test --locked -p chat-state --lib usage`：35 passed、0 failed（日志 `terminal/call_d2e83f2cba824a18a5298de15ddd6067.log`）。包含同 Turn 100+50=150、无新调用保留100、checkpoint与后缀、session-only、归属冲突及旧事实下界。恢复 coder 另执行全量 `cargo test --locked -p chat-state --lib`：670 passed、3 ignored；该次数仅为 chat-state 层证据，未代替 Shell 跨进程验证。
- 稳定归属 coder 的最终定向结果：`subagent_usage_fold` 19 passed、`record_response_token_usage` 13 passed、`usage_categories_tests` 2 passed、`sampling_attempt_delivery_tests` 1 passed；`cargo test -p tools --lib grow_build::task::` 222 passed。父随后亲读 TurnStarted ACK 后与 restored Turn 共用的 owner 资源安装段。
- 上下文通知已改为 `contextUsedTokens`，PromptResponse 顶层同义出口同步改名；真实 `usage.totalTokens` 不变。`cargo test --locked -p shell --lib prompt_response_meta_tests`：7 passed；`cold_goal_pressure`：3 passed；`session_usage_projection_is_transient`：1 passed。仓库内没有外部 Bot 的实现，不能据此宣称外部 Bot 已适配。
- review 新增不完整标记回归：现有 `mark_usage_incomplete` 对 prompt 仅内存标记，须在同一用量事实流携带稳定 owner 后再验收恢复完整性。相应修复独立推进，不以先前35项全绿掩盖此缺口。

### 当前整合回归

- 父统一运行 Shell Behavior、Plan 普通权限、history 与目录等 66 项：58 passed、8 failed（`terminal/call_5ef66eae1b4440e8b1f410ba42562eec.log`）。3 项 confirmed projection 与 Prepared 双向页通过。失败逐项分离后再验证，不删除场景：权限 fixture 把先命中 safe classifier 的 `pwd` 误期望为 policy_allow；3 项 retained Plan fixture 在失败 ACK 后复用已经停止的持久化 owner；目录 consume 后未揭示；around 目标靠近开头导致没有 olderCursor，后续两项为同 mutex 中毒连带失败。
- 权限 fixture 保留 `pwd` 的 safe_command，新增 `git remote -v` 的 policy_allow，五阶段四类决策共20个真实审计事件与5次 gateway prompt；`cargo test --locked -p shell --lib plan_phases_share_ordinary_bash_permission_decisions`：1 passed。生产权限规则没有为测试放宽。
- retained Plan 的失败和成功 ACK 分别使用新 actor 后，三个新增 durable adopt/notification 测试：3 passed（默认栈）。既有 `deferred_plan_decision_is_recovered_and_reapproved_from_control_before_tool_terminal` 在默认栈 SIGABRT、8 MB 栈通过；该项仍待判断，未把增大栈算作产品修复。
- around fixture 仍保留300行与首尾、大消息断言，把中段目标从 ordinal40移到200后，history paging 与 around trim：7 passed，包含 resident cursor-tail 保留 cut，原 mutex 中毒两项复跑通过。父亲读双向 seek、预算 trim 后重算 continuation 及共享 DTO。
- 父统一运行 Pager messages reducer、metadata、Plan 状态/回执与 resumed usage：133项中132 passed、1 failed（`terminal/call_8b5f5de027ff49c6a8a86e44a2f45723.log`）。messages 结果与 resumed_usage 全部通过。唯一失败的 phase fixture 在同 epoch/revision 注入不同值，改为四个独立 cold load 后 exact test 1 passed；同版冲突拒绝规则保持。root/child 旧 epoch、历史回执、等版精确回执和 hydration 不结算 intent 的入口测试通过。
- 父运行 indexed response、Tool、继承正文、rewind 与 completion paging 五项：3 passed、2 failed（`terminal/call_25289b105a424d6b8cd5090f69ed6bfb.log`）。response 损坏拒绝、rewind 同位置复用、completion 跨页通过；另外两项正在核对 oracle：双向分页允许不同分片切法，应各自完整重组；超大 Tool 的 terminal 权威也可能在 fragment header。未接受其候选，保留预算、坐标、digest 和原文一致性验收。
- 主窗口的跨片文字/Tool JSON 重组与 live 去重已越过原失败点；最后一个 bookmark fixture 锚在 entry 后 gap，已改成真实多行正文中的逻辑行，保留完整 bookmark 比较，未放宽为只比 EntryId。通用布局改动已撤回，原 gap 往返问题另记 backlog。该窗口最终集成测试仍待复跑；真实输入、时间线 UI、Minimal 与跨进程里程碑尚未完成。以上模块次数不代表整体完成，change 保持未归档、Git 未提交。
- 不完整 marker 修复后 `chat-state --lib usage` 为41 passed，全量676 passed、3 ignored；payload中的稳定 owner 决定活动账本归属，旧无payload标记只进入lifetime。Shell三个冻结fixture改为建立真实Turn，未构造无owner账本；父随后实跑19项 subagent usage fold 全部通过。
- 父核对 `docs/development.md:460` 后按已有 `RUST_MIN_STACK=16777216` 配置复跑：状态/Control/Plan恢复/权限/child错误路径25 passed（`terminal/call_a3371df87c254daa9a1efe7f4f75aeed.log`），含先前默认测试栈溢出的 retained Plan 原场景。原生 debug actor 已使用32MiB，release为8MiB；未修改产品栈。另以真实模块名运行挂起receipt/retry/waits10 passed（`terminal/call_2ac6a5eb892e4407b0caada1374d9b55.log`），不能把第一次仅匹配文件名过滤的零命中计为这组验证。
- published目录层确认 consume 后原fixture没有推进publication，补明确publication后3目录回归通过，`working_snapshot::tests`46 passed、1个既有手动大数据用例ignored。最终writer以consume返回positions直接读CHUNKS，普通连续consume保持同generation append；旧source可见性/branch变化才重建，已撤回全CHUNKS扫描及无条件reorder的猜测补丁。固定load cut还须合并未发布目录后缀，此缺口独立实施，不把大observation测试当作后缀已覆盖。
- 父亲读并接受两项分页oracle修正：双向各按canonical `(event,item)`重组并逐字节等于原文；Tool fragment header强核Completed/title/id/digest/total_bytes和连续区间后完整JSON一致。二者exact各1 passed，随后46项storage组一并通过。
- 意外递归rustfmt的6个无关文件已逐hunk恢复原格式，父再次核对这6路径 `git diff --stat`为空、`git diff --check`通过；没有checkout或整文件丢弃功能改动。
- 冻结结果 DISPLAY 回归强化后，父运行 `RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib published_turn_completion_presentation_is_paged`，1 passed（`terminal/call_da303d4530724e8d987150ad318837af.log`）。旧页中预构造的150 token快照在双向页与再次冷读后 typed/序列化全等，包含模型分项、缓存未知、未知费用和 incomplete；此测试不证明真实100+50的生成或后续session-only结算，不能代替PTY。
- 时间线候选定向8 passed，但父review补出两个接入缺口：已发around之后选择本地/live目标或关闭rail仍可能采用旧正文窗口；目录失败的TaskComplete末尾会再次ensure，可能立即重发。前者已独立派修，后者等待同模块写权释放后修复。主窗口页晚于同messageId实时用户回声时的重复实体也单独闭合，不能只skip导致用户提示排到其历史响应之后。
- PTY测试已准备两个真实Ctrl+D/新进程场景，尚未运行。父review纠正其oracle：Domain Timeline不能读取Presentation TurnCompleted，改用已验证的PersistedData.updates；Behavior检查最新control_snapshot；屏幕计量不得None==None；100与50必须关联同一稳定Turn/prompt owner。测试源码完成不计整链通过。
- 目录后缀最终版父侧实跑：原目录/插入顺序4 passed（`terminal/call_353aa9e81e884a25853bc7c30f7918f5.log`），实际 `suffix_overlay` 存储组5 passed（`terminal/call_feb28aa2ab6e4a59b6014e8dd7adf2ac.log`）。300条已发布+2条suffix样本中，around最早位置读128条rank，newest不超过128，before(10,4)读4条，非法ordinal读0条。capture时仅保有界metadata，query不重读正文。
- 父review发现 SEEN 的generation成员资格不等于旧span内成员资格：同代后来发布可在旧cut捕获期间加入新id。最终SEEN保存首次ordinal，以`rank < span.count`判断，交错回归通过。working manifest VERSION 51复用原派生索引失效/重建路径；已移除候选新增的删表回填与类型不符返回false支路，未删除journal。
- Minimal临时屏复查确认同batch双Ctrl+D可以跳过最后draw。清理责任须沿现有writer drain/terminal teardown闭合，不能只把browser设为Leaving；当前已交接实现，尚未用此项宣称真实PTY成功。
- Shell 最终库回归：`RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib -- --test-threads=4`，4862 passed、0 failed、14 ignored（`terminal/call_057c7c2e4c804664a4bd5583b489a3e4.log`）。包含目录后缀 VERSION 51、Behavior/Plan 保存恢复、稳定用量归属与完成结果留存；ignored 场景不计通过，进程级验收仍单独记录。
- 当前 CLI 构建后真实 `pager-pty-harness --test plan_approval_resume`：3 passed，覆盖直接恢复待审批、延期后恢复审阅及两轮修订批准完成（`terminal/call_2a1980eec524458687677949643f74bb.log`）。
- Pager 定向整合最终 46 passed，Minimal native commit 1 passed（`terminal/call_3773829a095e406fb73a4c3abde4e430.log`），含 20 项目录/跳转/失败门、13 项输入与边界、6 项 live 用户回声、主窗口合并与真实 pending lifecycle。夹具按实际 generation/selection/cache 分类分母修正；连续 assistant chunk 按正文顺序及唯一性验收，边界预取按准确请求数及 nonce 验收，未删除失败场景。
- 当前 CLI 的跨进程 100+50 同 Turn PTY 通过（`terminal/call_c35c2a3d62f04903b07fa7ca45817d56.log`）。首次运行另有未匹配辅助响应的 17 token 正常计入原轮，fixture 现显式给 auxiliary 一次零消费响应并验证被消费；主100、恢复后50共用原稳定 owner，终态150与再次恢复一致。该零消费 fixture 不代替非零 auxiliary/child 的后端归属回归。
- 多页 Plan PTY 已完成模式保存/两次冷恢复、两段历史与返回 live，但最后比较把 context 文本一并解析进 cache，导致 `N/A cache │ 8.0K / 128K` 与 `N/A cache │ 11 / 128K` 假差异。parser 现以状态栏分隔符截取真正 cache 项，仍保留 token、cache 和持久账单相等断言，待复跑。
- 全量 Pager 首轮：7708 passed、3 failed、15 ignored（`terminal/call_a2db6581b83646afa4d6464889454238.log`）。失败分别为首次 bind 清掉 response 前已接纳的控制 epoch、滚轮 fixture 受16ms cadence影响、目录 `tick_count` 命名触发既有动画架构检查；各自局部修复，未放宽保护或忽略测试。
- Minimal 真实 PTY 首次运行暴露初始 Latest 页被提前全部标 committed，使 native 输出只剩输入区（`terminal/call_15f2cbd28c844de785e695e51786e97f.log`）。按首次 Latest/后续历史的原职责边界修复；设计和 delta 已补首个最近页提交场景，尚未据此宣称 Minimal 里程碑通过。
