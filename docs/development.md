# 开发流程

项目使用 OpenSpec SDD。先把本次要改变的行为写清楚，再实现和验证，最后归档成为当前规范。纯文档修正也保留最小变更记录，但不增加无意义需求。

终端工具活动的分组入口是 Pager 的 `scrollback/state/groups.rs` 与
`verb_group.rs`：先从原始顺序确定活动范围，再统计整组摘要并投影可见行。
工具类型、运行终态和成员正文展开不改变组身份；`EntryLayoutInfo.unfolded_height`
保存成员原生高度，`height` 保存当前投影高度，重复布局从原生高度重算。
排查实时更新时同时核对首成员的摘要/正文行偏移、`expanded_groups`、单项 fold pin，
以及 live 与 batch replay 的布局一致性。活动聚合启用时，工具与 Thought 的手动展开
由活动展示直接保留，无需另开 `respect_manual_folds`。行为见
[工具与 Thought 聚合](../openspec/specs/client-surfaces/spec.md#requirement-continuous-tool-and-thought-activity-forms-a-stable-group)。

模型目录在配置解析时把 provider 的 `api_backend` 作为默认值投影到各 model；
model 显式值优先，最终采样配置只持有解析后的协议。配置到请求的边界见
[model-sampling](../openspec/specs/model-sampling/spec.md#requirement-model-api-backend-overrides-its-provider-default)。

采样断流排查从 attempt evidence 的 `attempt_number`、`stream_end`、
`output_delivery`、`output_observed` 和 `recovery_stop` 决定开始，再核对
Timeline 的 `sampling_usage/attempt_settled`。`[DONE]` 或 body EOF 不能代替
协议完成证据；没有对应会话的原始尾帧时，不能仅凭错误文字归因于代理。
HTTP 200 的 SSE `event:error` 要连同事件名核对；平铺的 `code/message/request_id`
是 provider 错误事实，普通事件缺少 `type` 则仍是协议错误。内容检查拒绝不自动重试，
限流和过载仍须通过统一 attempt 安全与预算门槛。契约见
[named stream error](../openspec/specs/model-sampling/spec.md#requirement-named-provider-stream-errors-retain-their-error-facts)。
切换模型后的图片恢复沿用共享能力拒绝分类器：含图片的 HTTP 400 若明确声明 `messages.content.type 参数非法，取值范围 ['text']`，与英文 text-only 声明一样触发现有视觉辅助、OCR 和持久化图片投影；投影确认后才重建请求。冷恢复先从已校验的 completed Sideband 复用同一原始图片的中性描述，辅助配置不可用也不跳过复用；无关输入引起的全局修订变化不使描述失效，但来源、prompt、完成证据及新投影的当前修订/指纹仍须验证，未来修订不接受。取消会关闭原 foreground 的派发资格并终止生产任务，不能以普通辅助失败继续 OCR、后续图片组或主请求。拒绝记录使用实际请求的 model、backend、endpoint/query 身份；catalog 别名相同但请求路由变化时重新尝试，不同别名指向同一请求路由时共享拒绝记录。辅助视觉模型每次按当前配置解析，并避免向已拒绝图片的同一运行时递归请求。分类器同时供 sampler 的拒绝用量结算与 Shell 恢复使用，不能只在一侧加入特例；其他类型列表或一般参数错误保持原路径。见 [图片能力拒绝与恢复](../openspec/specs/model-sampling/spec.md#requirement-session-image-descriptions-retain-original-media)。
Chat usage 尾帧的 audio、reasoning、prediction 明细可缺失或为 `null`，wire 类型保留为未知；显式零仍保留。可信总量和缓存字段照常归一化、结算，reasoning 未知时沿现有内部统计边界记为零。必需总量为空或明细类型非法仍拒绝整帧，不能靠跳过错误恢复。见 [可空 Chat 明细](../openspec/specs/model-sampling/spec.md#requirement-chat-usage-accepts-null-optional-token-details)。
当前 `GROW_MAX_RETRIES` / `max_retries` 沿用字段名，表示同一未接纳模型步骤的
总 attempt 上限，包含初次调用；0 仅允许初次调用。session 默认上限为 5，
独立 sampler 默认值为 15。共享期限取首次 idle timeout × 总上限，后续修复
不会延长；精确预算或不可撤销输出可能更早关闭恢复。
自动恢复耗尽后，失败 Turn 通过正常终态结算释放前台；同一 resident session 的后续用户输入建立新 Turn 和新的逻辑采样额度，有效历史与已结算消费继续保留。子任务输出 grant 按 attempt 的真实用量扣减，已知消费不能仅因采样失败变为未知或耗尽；未知用量、结算失败及真实额度耗尽仍关闭该子任务的准入。Goal 内部失败会暂停自动续跑；暂停或预算受限后，普通“继续”仍可发起 Goal 外的用户回合，但不会重启 Goal、追加它的消费或清除原有用量问题。使用既有 `/goal restart` 激活，预算耗尽或精确用量不明仍须按原控制流程解决。见 [失败后继续的验收契约](../openspec/specs/model-sampling/spec.md) 与 [Goal 恢复边界](../openspec/specs/behavior-goal/spec.md)。
真实跨层回归先构建本次 `grow`，再设置 `GROW_BINARY` 指向该产物，运行 `cargo test --locked -p shell --test test_session_protocol_recovery -- --ignored --test-threads=1`；覆盖三个协议的重试耗尽、同 actor 手动继续及 Goal 恢复。核心 library 回归中的 `sampling_failure_recovery_tests` 使用生产 session run loop，受控延迟失败 Turn 的持久化 ACK 后发送新输入，核对队列排序与子任务 grant；该夹具模拟写入确认，实际磁盘持久化由前述真实二进制回归覆盖。
模型切换后的 thinking 400 需核对错误要求的是 Chat Completions `reasoning_content` 还是 Responses `reasoning_text`。HTTP 与 SSE 的结构化错误在展示截断前根据明确回传或缺失字段要求生成 typed backend 事实，再交给 ChatState；只有匹配的当前路由可确认启用一次，DTO 往返不从短文案重新推断。请求证据的 `portable_reasoning_backend` 记录这一选择。Chat 将紧邻 assistant 的可见 reasoning 编码回传，无文本时传空字符串；Responses 回放普通非空回答或完整工具往返前紧邻的已有非空可见 reasoning；没有文本不虚构空 reasoning item。可见摘要不等同于完整原生思考，线上目标接受性仍须核验。原生签名、加密内容不从历史重建，同路由 reset 保留已学习规则，真实路由替换清除规则。契约见 [portable reasoning 恢复](../openspec/specs/model-sampling/spec.md#requirement-provider-required-portable-reasoning-recovery-is-bounded)。
实现导航见 [采样恢复边界](architecture/session-robustness-repair.md)，
行为以 [model-sampling](../openspec/specs/model-sampling/spec.md) 为准。
`sampling_evidence/recovery_stop` 是停止自动恢复的合法历史证据，与 request、response、retry 一起经过加载及导入导出的完整性校验；不能删除该记录来绕过加载失败。契约见 [停止恢复证据](../openspec/specs/session-timeline/spec.md#requirement-sampling-recovery-stop-evidence-survives-session-storage)。
Sampler graceful shutdown 会先停止准入并取消 provider 工作，再等待已准入 request task 完成 attempt evidence 与所有适用用量账本的确认结算；不能用 abort 或缺失 ACK 跨过该边界。若现有 `SamplerOwner::shutdown_bounded` 的明确 deadline 到期，owner 才会强制终止剩余任务并返回关闭失败，调用方不得把它当作成功的最终持久化 frontier。

子 Agent cancellation token 只发起取消，不能直接授权写入关闭 child Timeline 的 `SubagentResult`。runner 必须等待 child prompt terminal，使已准入 attempt 的 evidence 与 usage settlement 先越过上述 frontier，再提交 child result 和 parent `Ended.result_ref`。完整且严格链接的 `cancelled` lifecycle 可以显式 resume；resume 先拒绝仍由 coordinator 持有的 source，再校验 parent/child storage、seed、identity 和 exact result link。恢复沿用 source 自己的稳定 System head，不依赖当前 head renderer 或历史 completion output artifact；真正被 Surface 引用的 prompt blob 仍须存在。派生 child 只有在 control snapshot、Goal snapshot mailbox（适用时）和首轮 prompt durable ACK 依次通过后才完成启动准入。契约见 [cancelled lifecycle 恢复](../openspec/specs/session-timeline/spec.md#requirement-canonical-cancelled-subagent-lifecycles-are-resumable) 与 [取消结算屏障](../openspec/specs/model-sampling/spec.md#requirement-subagent-cancellation-settles-admitted-attempts-before-closing-the-child-timeline)。

Assistant response admission 由 ChatState Timeline 持有 `request_id + final attempt` identity 和确定性的 quarantine 结果。Shell 将 identity/payload 交给当前 ChatState owner；owner acknowledgement/persistence/causal 错误直接以 `response-admission` turn-boundary fatal error 停止 completion recovery、Accepted 发布和工具执行，不通过失效 owner 重试。cold/replacement owner 可用后，exact identity/payload reissue 幂等返回原结果，不重启 sampler/provider、不重复安装 native continuation。历史 response 的可选 metadata 缺失时仍可读取，但不能用启发式确认新的不明 admission；native continuation 仍为瞬态状态。

Identity-bearing response 以唯一 Timeline admission 的 durable ACK 为 Accepted 与后继 provider/工具的边界，不再提交独立 response projection。canonical assistant/reasoning 正文只存 admission，History projector 在生成 ACP 通知时复制展示文本。流式 candidate 只用于可撤回 live preview；首个 candidate 在统一物理日志中留下无正文 typed anchor，admission 绑定该位置，独立事实保持原到达顺序。每会话预览仍受 4096 个 16 KiB credit 约束，直到网关完成才释放；超限、anchor 或独立持久事实失败仍阻止该次 admission。Shell 在 admission 前刷新 event FIFO 并等待持久化屏障；attempt 中的独立 ACP/Grow 更新先冲刷前件、确认写入后才发布，不在 sender 中无界堆积。恢复从同一已验证日志截点派生 canonical 历史，不修补第二份磁盘 cache；quarantine 与无 admission 的 candidate 不获得 accepted 或执行权威。契约见 [session-timeline](../openspec/specs/session-timeline/spec.md#requirement-candidate-preview-uses-a-durable-payload-free-anchor)。

普通会话的 Sampler→Shell 事件交接对 candidate 文本、reasoning、工具参数和响应元数据片段使用每会话 4096 个 16 KiB credit；单个片段超过 64 MiB 会在入队前使 attempt 失败。会话 actor 消费该片段翻译出的事件并确认 FIFO fence 后才归还 credit，因此忙碌的会话事件循环会反压 provider 读取，取消仍可打断等待。ReplayBuffer 的可配置合并阈值被限制在片段预算以内。attempt 边界和终态保留原有 FIFO 顺序；Sampler→Shell 与 Shell→gateway 是独立预算，不等同于进程总内存上限。leader 对不支持撤回的观察者只暂存 candidate 通知；独立 ACP/Grow 通知照常实时投递，因此 live 展示可早于随后接纳的候选，而 durable replay 保留原顺序。该候选按每会话单个临时 spool 保存：8 MiB 内存阈值后落到 socket 同目录的匿名文件，每个候选最多 512 MiB 序列化记录或 100 万条，Accepted 逐条回放，Discarded 直接释放；超限或 I/O 失败会丢弃该 attempt 的临时候选，保留订阅和独立通知；若随后 Accepted，leader 在在途 load 完成后通知 Pager 原地全量回放，Discarded 则无需重载。见 [采样片段交接契约](../openspec/specs/model-sampling/spec.md#requirement-session-sampler-preview-handoff-bounds-fragment-backlog)、[单片段超限契约](../openspec/specs/model-sampling/spec.md#requirement-oversized-sampler-preview-events-fail-before-enqueue)、[独立通知契约](../openspec/specs/client-surfaces/spec.md#requirement-independent-notifications-bypass-retractable-candidate-buffering)和[候选暂存契约](../openspec/specs/client-surfaces/spec.md#requirement-leader-candidate-retention-has-a-finite-spool-budget)。

Goal 与 Workflow 的 Grow 展示快照同样使用该会话网关预览 credit，并在入持久化队列前为其独立副本占用 credit，分别在网关完成和写入完成后释放；慢消费者导致的预算耗尽会跳过展示快照，若发生在活跃 attempt 内则阻止该次 response admission。Goal 控制快照与 Workflow manifest 的权威持久化不依赖这些展示快照。子 agent 的瞬态进度发布者每次只等待一条网关投递，取消可打断等待。契约见 [辅助 Grow 投递](../openspec/specs/session-timeline/spec.md#requirement-auxiliary-grow-delivery-shares-the-session-preview-budget)。

子 agent 的 Spawned/Finished Grow 生命周期通知由父会话 actor 在 observe hook 完成后分配 `eventId`，写入后再转发，写入与实时通知复用该 ID；Goal-owned Spawned 先转发，再发布由它触发的 Goal 状态刷新。其他生产者仍可能在持久化 ACK 等待期间抢先到达，Pager 因此按 child identity 接纳迟到且未应用过的 Spawned/Finished，保留较新的事件高水位和 reconnect cursor；已显示终态的 child 不会被迟到的 Spawned 重新标为运行。Spawned 还从 durable spawn fact 投影实际 reasoning effort，Pager 将其显示在 Tasks 列表和打开的子 agent 标题，并跟随子会话的模型切换更新。父 actor 不可用时，生产者为直接网关回退分配 ID。Finish 展示只携带状态与计数，不复制完整答案。答案保存在经过验证的子会话结果和父会话完成回执中。父 actor 在活跃 attempt 内转发时沿用独立事件 ACK 与预览网关预算；外部客户端送入的 Grow 通知仍只持久化，不回显。契约见 [子 agent 生命周期投影](../openspec/specs/session-timeline/spec.md#requirement-subagent-lifecycle-projections-are-bounded-metadata)、[effort 投影](../openspec/specs/session-timeline/spec.md#requirement-subagent-spawn-projections-retain-effective-reasoning-effort)及[客户端展示](../openspec/specs/client-surfaces/spec.md#requirement-subagent-model-labels-show-effective-reasoning-effort)。

输入栏的模型标签默认使用 catalog display name；`[ui].show_model_provider = true` 后改用当前 canonical `provider/model` ID，实际 effort 仍显示在括号中。Settings → Models 的 “Show model provider” 可用 Enter、空格或点击值切换；它通过现有配置写入及 Pager render cache 即时作用于普通会话、子 agent 和 Dashboard，写入失败沿设置回滚路径恢复。契约见 [输入栏模型标签](../openspec/specs/client-surfaces/spec.md#requirement-prompt-model-label-optionally-includes-provider)。

工具通知桥的 TaskCompleted Grow 投影清空 `TaskSnapshot.output`，完整后台输出仍由任务文件和限长的模型通知读取；TaskBackgrounded、TaskCompleted、ScheduledTaskCreated 的持久化/网关副本及 ScheduledTaskFired、MonitorEvent 的实时副本共享该会话预览 credit。要求持久化回执的完成事件先确认 Grow 写入，再发送实时展示。契约见 [工具桥 Grow 预算](../openspec/specs/session-timeline/spec.md#requirement-tool-bridge-grow-projections-share-preview-credits)。

重复 rewind 保留原始 branch leaf 和 response admission 来源，compaction 后也按未压缩前缀选择历史。普通 fork 从父统一 Timeline projector 派生历史，正文引用新会话 seed，父 request/attempt 不成为子 admission。Resident reconnect 由原 actor 持有 writer barrier 固定完整前缀，恢复 live 后才放行后续提交；屏障期间启动的 attempt 也登记完整正文补发，客户端只替换同 attempt 的 provisional 行。Cursor 绑定 session、projector 版本、完整前缀 hash 和展开位置；半个 response 或新截点走完整替换。Exact 事实提交即使记录已可读，也须完成原 file/directory barrier 后才能 ACK。契约见 [rewind 来源](../openspec/specs/session-timeline/spec.md#requirement-repeated-rewind-preserves-retained-response-provenance)、[exact 持久确认](../openspec/specs/session-timeline/spec.md#requirement-exact-replay-commits-acknowledge-durable-barriers)、[fork 历史](../openspec/specs/client-surfaces/spec.md#requirement-forked-response-history-belongs-to-the-new-lineage) 和 [resident 回放截点](../openspec/specs/client-surfaces/spec.md#requirement-resident-replay-preserves-the-physical-snapshot-frontier)。

回退历史由 pinned 文件句柄延迟读取。picker 扫描 metadata 时校验嵌套 snapshot 的类型但不保留正文，实际执行再读取完整内容；两者的单条记录上限为 64 MiB，单次扫描上限为 256 MiB / 50,000 条。读取运行在 blocking pool，取消后 worker 继续持有该句柄的读取锁直到退出，失败保留来源供重试。metadata 失败通过 `grow/rewind/points` 报错，客户端关闭当前选择并恢复草稿，不能把失败解释为零文件改动。行为见 [回退读取预算](../openspec/specs/client-surfaces/spec.md#requirement-pinned-rewind-history-reads-have-bounded-and-visible-failure)。

回退执行的异步结果带来源 session ID 和绑定 epoch；仅当前绑定能截断 transcript、恢复草稿或触发 inline edit 重发。解绑或换绑会关闭旧回退界面，把未发送的草稿及编辑文本留在本地输入框；迟到的明确成功、拒绝或未知结果只提示原会话状态，成功后应重新加载原会话核对。行为见 [回退执行归属](../openspec/specs/client-surfaces/spec.md#requirement-rewind-execution-feedback-belongs-to-its-source-session-binding)。

## 环境

Rust toolchain 由 `rust-toolchain.toml` 声明，构建仍使用 Cargo。OpenSpec 是开发工具，不进入 Cargo 运行时依赖。

```sh
npm install -g @fission-ai/openspec@1.11.0
openspec --version
openspec list
openspec list --specs
```

本仓库通过根 AGENTS.md 和 CLI 接入，未提交个人 AI 工具配置。下面的 CLI 命令不要求 `/opsx:*` 已安装；需要工具专属交互时可自行运行 `openspec init --tools <tool>`，不要覆盖项目规则。

## 一次行为变更

1. 读取相关 spec、实现、调用方和测试。若代码与 spec 不一致，在 proposal 说明发现与处理方向。
2. 建立 change，按 CLI 返回的上下文逐项写 artifact。

```sh
openspec new change fix-example --schema spec-driven
openspec instructions proposal --change fix-example
openspec instructions specs --change fix-example
openspec instructions design --change fix-example
openspec instructions tasks --change fix-example
openspec status --change fix-example
openspec validate fix-example --strict --no-interactive
openspec instructions apply --change fix-example
```

`fix-example` 是占位名称。每个 change 包含 `proposal.md`、`design.md`、`tasks.md` 和 `specs/<capability>/spec.md` delta。修改已有要求用 `## MODIFIED Requirements` 并保留该要求的完整正文和全部仍成立的场景；新增用 `## ADDED Requirements`，删除写原因及迁移影响。

```markdown
## ADDED Requirements

### Requirement: Example behavior
系统 SHALL 在明确条件下产生可验证结果。

#### Scenario: 明确边界
- **WHEN** 触发具体条件
- **THEN** 观察到具体结果
```

3. 按 tasks 实现，更新对应 docs 解释，执行与影响范围匹配的检查；验证记录放在 change 的 `verification.md`。需要验证正常、错误或恢复路径时使用有意义的测试。未执行、失败、外部条件缺失必须写清楚。
4. 确认场景落实后勾选 tasks，归档同步主规范，再次校验。

```sh
openspec validate --all --strict --no-interactive
openspec archive fix-example --yes
openspec validate --all --strict --no-interactive
openspec validate --archived --no-interactive
```

归档是维护规范的本地操作，不代表 Git 已提交、合并或发布。CLI 的 artifact 状态只检查文件是否存在；格式通过也不能证明行为正确。

## 离线 transcript 调试

离线 transcript 的实现入口是 `shell::session::storage::transcript`、`pager::transcript_projection` 和 `pager::replay_cmd`。排查 export/replay 时分别核对捕获来源、事实到展示的转换、虚拟时钟；不要通过构造 SessionActor、恢复 executor 或运行 Hook 来补历史。`TranscriptSnapshot` 固定所选 ledger 的文件身份和读取上界，跨 session 仍不是全局原子快照。取消/下一输入不代表 pending 工具成功，晚到 terminal 必须按 prompt identity 处理。相关行为见 [离线中断归属](../openspec/specs/client-surfaces/spec.md#requirement-offline-transcripts-preserve-interrupted-turn-ownership)、[来源截点](../openspec/specs/client-surfaces/spec.md#requirement-offline-snapshot-frontiers-survive-normal-append) 与 [播放语义边界](../openspec/specs/client-surfaces/spec.md#requirement-replay-semantic-boundaries-take-precedence-over-simulated-text)。

Behavior 选择、Plan phase、Goal 与 foreground turn/Workflow Run 分别投影。取消 turn 不隐式暂停 Goal，离开 Workflow Behavior 不隐式终止 Run；模式和控制回执可以穿插模拟正文，不触发执行或提前结束正文。reader 校验 Timeline 的 `SessionControlSnapshot`，如果显示缓存遗漏最后状态，在末尾追加明确标记的截点状态；不能据此还原缺失的中间确认界面或事件位置。Goal clear 清除当前目标展示，停止状态仍允许已记录的重启，Workflow 正 revision 防止旧更新覆盖新投影。契约见 [离线 Behavior/Goal](../openspec/specs/client-surfaces/spec.md#requirement-offline-behavior-and-goal-projections-remain-observational) 与 [Control 截点](../openspec/specs/client-surfaces/spec.md#requirement-offline-control-snapshots-use-captured-authority)。

定向回归可运行 `cargo test --locked -p shell --lib session::storage::transcript`、`cargo test --locked -p pager --lib transcript_projection`、`cargo test --locked -p pager --lib replay_cmd` 和 `cargo test --locked -p pager --lib export_cmd::tests`。`replay_cmd` 不带 `::tests`，以覆盖生产 tree/browser/time/idle/panel 子模块。真实 CLI/PTY 的离线中断 smoke 与未覆盖范围保存在 [中断审计记录](../openspec/changes/archive/2026-09-29-harden-transcript-interruption-snapshots/verification.md)，Behavior/Goal 组合覆盖见 [专项审计](../openspec/changes/archive/2026-09-29-harden-offline-behavior-goal-projections/audit.md)。

### 只读 Replay

Replay 的独立 TUI 复用普通 AgentViewLayout、顶部状态栏、TasksPane、ScrollbackPane、详情 viewer、历史 turn 状态和 ShortcutsBar。仅将 composer 槽位换成三行播放控制框：状态/倍速/进度、播放操作、历史时间/回放耗时；诊断信息放在帮助中。任务栏沿用普通自动展开/收起和 `h` 查看完成项，`t` 切换任务列表；子 Agent、Workflow 关联 child 和已捕获后台输出可只读查看。共享展示只使用当前已交付事实，未知字段不从当前运行时补取；任务/Goal 计时走历史来源时钟，暂停冻结，IDLE 压缩不改变业务耗时。入口和场景见 [Replay 布局契约](../openspec/specs/client-surfaces/spec.md#requirement-replay-panel-replaces-live-composer-and-status-controls) 与 [任务状态契约](../openspec/specs/client-surfaces/spec.md#requirement-replay-task-and-status-surfaces-follow-delivered-history)，布局验收见 [记录](../openspec/changes/archive/2026-10-01-align-replay-with-session-surfaces/verification.md)。

F8 可在任意焦点暂停，`]` 在正文或通过控制框显式跳到下一记录；Finished 保持可浏览但没有继续动作。已确认且严格超过 30 秒的 IDLE 压缩为 1 秒播放时间；中断恢复空档标估算，普通工具中无法确认的权限等待和缺时后台任务保守保护。旧记录缺少的时间与等待事实不能从静默或当前时间推断。只读交互、时区和 IDLE 验收见 [前序记录](../openspec/changes/archive/2026-09-30-interactive-readonly-replay/verification.md)。

### 普通 resume 历史

普通 resume 的历史加载与 `grow replay` 播放器是独立入口。`session::storage::communication_history` 将 Timeline-only 通信按 canonical response、input、tool 和已有通信身份定位到历史区间，根 load、延迟 child 历史和离线 transcript 共用该规划；加载后的 coordination snapshot 只发布当前待处理 inquiry。已有 receipt/阶段保持原位置，缺失接收方 inquiry 阶段按 source peer + inquiry ID + phase 去重。合成行保留来源时间且没有缓存 eventId，不能充当 reconnect cursor；涉及合成时沿完整替换回退。没有可靠锚点会标估算，缺失正文使用既有 unavailable 说明。它不重新投递消息、消费通知或运行 Hook；不新增 viewport 持久化，也不改变普通 Goal 恢复策略。契约见 [恢复通信顺序](../openspec/specs/client-surfaces/spec.md#requirement-resume-restores-ordered-communication-history)，验证见 [记录](../openspec/changes/archive/2026-10-01-restore-ordered-resume-communications/verification.md)。

## 性能验证记录留存

性能变更归档仅提交最终可信结论：在 `verification.md`（纯分析可用 `analysis.md`）写明最终数值、样本与环境、比较口径、未改善项、失败或不确定性以及正确性验证结果。逐次运行日志、trace、终端录屏、构建输出、临时脚本、热身和中间候选不进入仓库归档；归档前检查报告引用不指向已清理文件。原始数据如需临时复核，在工作区外保存，不能用其存在作为最终结论的前提。

## 不改行为的最小变更

先创建 change，在生成的 `.openspec.yaml` 中保留 schema/created 并加入 `skip_specs: true`。proposal 写明不改行为的原因，design 简述影响，tasks 只列必要动作和验证。不创建空 delta，也不为工具维护虚构产品能力。完成后使用 `openspec archive <change> --skip-specs --yes`，再运行上述全量与归档校验。

Pager 列表布局缓存的每项几何查询以 `Option` 表达缓存范围外的索引；增量追加同时支持固定行高和变高布局。契约见 [列表布局缓存边界](../openspec/specs/client-surfaces/spec.md#requirement-list-layout-queries-respect-cached-item-bounds)。

Pager 子 Agent 权限审计组按 primary-turn epoch 聚合。成员追加与 reconnect 合并由 `ScrollbackState` 持有 epoch 并核对组的来源 epoch；`SubagentPermissionBlock` 不向 crate 外开放成员 mutation。契约见 [子 Agent 权限组 epoch](../openspec/specs/client-surfaces/spec.md#requirement-subagent-permission-groups-preserve-their-source-epoch)。

Minimal 每帧从当前 root、选中子 Agent 和 root 权限队列解析唯一可见 Agent；commit、viewport、live 和输入均使用该归属。切换视图会清空当前屏幕并重建新视图的原生提交边界，旧内容仍保留在终端历史中。实现入口为 `pager::minimal_api` 与 `pager-minimal::draw`；行为以 [Minimal 视图归属](../openspec/specs/client-surfaces/spec.md#requirement-minimal-frames-render-the-selected-agent-view) 为准。

Minimal 新会话欢迎卡在 root Agent 绑定 session ID 后才提交，因为绑定会改变可见视图归属并清空 placeholder epoch 的当前屏幕。卡片会在该会话首批对话条目之前写入原生终端历史；行为以 [Minimal 欢迎卡](../openspec/specs/client-surfaces/spec.md#requirement-minimal-welcome-card-belongs-to-a-bound-session-epoch) 为准。

## Rust 验证入口

Bash 持久命令沿 `ShellState::prepare_command` 的 fd 3/4 传输快照；dump 在声明任何局部变量前捕获 `export -p`，原始 allexport 标志保存在函数自身位置参数中，避免序列化变量遮蔽用户同名导出值。排查大函数与 `set -a` 时，核对临时值的 export 属性及恢复顺序，不能靠缩短函数或关闭用户选项消除失败。回归入口为 `computer::local::shell_state::tests`，行为见 [Bash 快照导出隔离](../openspec/specs/tool-authorization/spec.md#requirement-persistent-bash-snapshots-isolate-serializer-export-state)。

快照 reader 必须等到完整结束标记行（含换行），并按固定后缀检查，避免大载荷反复全文扫描。背景进程仅持有 fd 4 不应阻止完整快照返回；五秒期限与不完整快照保留旧状态的语义不变。见 [快照结束边界](../openspec/specs/tool-authorization/spec.md#requirement-shell-dump-readers-require-complete-terminator-lines)。

ConfigFileWatcher 在 debouncer 回调处理路径时重新核对配置源所有权，已进入接收队列的通知不撤回。leader 与进程内 runtime 通过相同的 session 登记消息处理监听；登记绑定现有 resident 资源，actor 重建期间保留，移除、空闲卸载或 agent 销毁时发送释放。失败或取消的重建若未发布新 resident，由现有生命周期 guard 释放留下的登记。runtime 消费释放后才回收无主路径；共享 cwd、仓库祖先、启动 cwd、全局和显式配置源保留各自所有权。路径在接纳时规范化，释放不重新解释已消失或改指向的别名；watcher 启动失败会关闭未消费的登记接收端。生产统一走 session 所有权入口，直接 watch/unwatch API 的调用方仍须自行协调全部引用。见 [配置事件所有权](../openspec/specs/configuration-rules/spec.md#requirement-configuration-watch-delivery-retains-a-live-source-owner) 及 [resident 监听契约](../openspec/specs/configuration-rules/spec.md#requirement-dynamic-project-watches-follow-resident-session-ownership)。

Agent 激活必须将新的 ToolBridge 绑定到原 SessionActor 的 Planning runtime，并从 Timeline 恢复局部计划投影。启动 hydration 与 Agent 替换共用该绑定；替换本身不提交新计划或重复发布 ACP plan。Plan 顶部阶段入口依赖 Behavior 投影，点击沿 canonical artifact 查询显示完整计划；TODO 徽标及面板独立保留。契约见 [Agent 替换与 TODO](../openspec/specs/task-planning/spec.md#requirement-agent-replacement-preserves-session-planning-access) 和 [Plan 顶部详情](../openspec/specs/client-surfaces/spec.md#requirement-active-plan-has-an-inspectable-top-status-entry)。

Plan 完成时先保全原始报告，再将完成引用与退出 Normal 一起提交到 Control；提交确认后才发布完成标记并继续正常采样生成简短总结。canonical 查询同时读取计划与报告，Pager 用独立 Tab 展示；完成入口只在下一次成功手动选择 Behavior 时清除，同模式选择也适用。恢复和自动 Goal 控制保留该引用，fork 不继承完成展示。契约见 [完成结果持久化](../openspec/specs/behavior-goal/spec.md#requirement-plan-completion-retains-an-inspectable-durable-result) 与 [完成详情和总结](../openspec/specs/client-surfaces/spec.md#requirement-completed-plan-details-separate-the-full-report-from-the-summary)。

Plan handoff 的 notification source version 使用 session 单调 submission ID，与每个计划周期的 artifact revision 独立；Timeline 校验、live admission 和冷 replay 必须保持同一身份口径。hash/revision/kind 仍属于 source/owner 身份，不能按旧文档版本去重新的提交。见 [Plan handoff 提交身份](../openspec/specs/behavior-goal/spec.md#requirement-plan-handoff-receipt-versions-identify-the-submitted-plan)。

Plan 要求修改后，Drafting 与 Amending 的 PlanPhase 模型上下文都携带当前计划及本次审批反馈；live 工具结果和 idle handoff 只负责交接，不另存反馈副本。重新提交新 artifact 后清除旧 handoff，恢复沿用已持久接纳的上下文。排查“仍按旧计划修改”时应核对模型上下文，不能只确认 Control 中存在 feedback。见 [修订反馈投影](../openspec/specs/behavior-goal/spec.md#requirement-plan-revision-context-carries-the-accepted-review-feedback)。

Plan 决定首次查询可能早于 Control 接纳并返回 unconfirmed。成功接纳后，既有 CurrentModeUpdate 携带 `grow/planDecisionReceipt`，Pager 与手动状态查询共用精确回执核对后关闭对应确认界面；不能根据 phase 变化猜测决定成功。不同提交的新审批释放旧 pending 的界面占用，旧回执不会关闭新审批。见 [决定确认交接](../openspec/specs/client-surfaces/spec.md#requirement-accepted-plan-decisions-release-confirmation-ui-without-polling)。

Plan 操作栏与 Minimal 控制条复用 `modal_window` 的分行测量和渲染，内容高度必须扣除实际 footer 行数。Minimal 审批正文保留在 native scrollback，普通/retained/完成详情则复用 line viewer；输入 owner 和可见 surface 必须一致。回归入口是 `pager-pty-harness --test plan_approval_resume`，连续修订场景同时核对下一次模型请求中的批注/自由文本和完成报告的持久 artifact。键鼠与窄屏契约见 [Plan 操作可发现性](../openspec/specs/client-surfaces/spec.md#requirement-plan-actions-remain-discoverable-across-layout-and-focus-changes)，普通预览批注仍按 [聊天消息语义](../openspec/specs/client-surfaces/spec.md#requirement-casual-plan-comments-disclose-ordinary-prompt-delivery) 发送或排队。

离开 Plan 时，先将 coordinator 选择语义应用到待持久快照并等待 Control 确认，再更新 live Behavior；保留 retained candidate 与 submission counter，清除 Plan runtime 字段。只有成功的手动 Behavior 选择按既有规则清除 completed-plan 引用；GoalLifecycle 选择保留它。冷恢复后下一次提交继续使用更大的 submission identity。压缩完成只更新压缩状态，root 和 child 的 TODO 投影都保留到显式 Plan 更新。见 [retained Plan owner](../openspec/specs/behavior-goal/spec.md#requirement-retained-plans-remain-under-the-existing-session-owner)、[completed Plan 持久结果](../openspec/specs/behavior-goal/spec.md#requirement-plan-completion-retains-an-inspectable-durable-result) 与 [局部计划的 live/resume 投影](../openspec/specs/task-planning/spec.md#requirement-committed-local-plans-have-one-live-and-resumed-projection)。

图片恢复仍是单条可见 operation row：root 与 child 各自投影到所属 scrollback；child 历史恢复包含持久 Grow lifecycle 和 prompt terminal，resident reconnect 的 cursor tail 按 operation identity 更新旧行并保留 EntryId。检查重连时同时核对重复行、终态和迟到 progress，不把恢复行移到 parent transcript。见 [图片恢复行](../openspec/specs/client-surfaces/spec.md#requirement-image-recovery-is-one-visible-operation-row)。

工具 runtime 基础设施错误必须停止当前采样与 completion recovery，并在原有 terminal transaction 中按 Tool、Step、Turn 顺序收尾；不跨越未确认工具提交 StepEnded，也不把 terminal ACK 失败当作成功。子会话 handoff 的历史优化可以复用已通过 `load_light` 校验的 Timeline，但仍需保留 child spawn/terminal 一致性检查和 verified-prefix reconciliation。见 [工具失败终态展示](../openspec/specs/client-surfaces/spec.md#requirement-generic-tool-failures-render-their-terminal-content-once)、[工具运行时失败边界](../openspec/specs/model-sampling/spec.md#requirement-tool-runtime-failures-stop-dependent-sampling) 与 [子会话 verified commit prefix](../openspec/specs/session-timeline/spec.md#requirement-subagent-reconciliation-advances-only-verified-commit-prefixes)。

按受影响 crate 缩小检查范围；下列命令是项目现有 CI/README 的入口，不意味着每次纯文档修改都运行全部测试。

先核对 `cargo --version` 和 `rustc --version` 与 `rust-toolchain.toml` 一致。该文件由 rustup 代理读取；若 Homebrew 的 Cargo/Rust 位于 PATH 前面，需要把 rustup 代理目录（通常是 `~/.cargo/bin`）置于前面并重新核对版本，避免本地验证与发布工具链不同。

```sh
cargo fmt --all -- --check
cargo check --locked -p cli
cargo test --locked --lib -p chat-state -p sampling-types -p sampler -p memory -p workflow -p shell -p pager -p pager-minimal -- --test-threads=4
cargo build --locked -p cli --bin grow
```

发布前分别执行默认 `cargo check --locked -p cli --bin grow` 和 `cargo check --locked --workspace --all-targets`，同时核对编译诊断与 build-script/linker stderr。库测试及 all-targets 会启用 dev-dependencies，不能替代默认生产构建；仅在测试图中可见的类型引用可能让测试通过而 CLI 无法编译。现有 core 门禁保留独立的默认 CLI build。

PR 的 `core-regression` 在构建 `grow` 后串行运行工作流列出的 Pager PTY、挂起和计划恢复回归，包括 minimal、设置、滚动选择、队列取消、真实 `less` transcript 恢复和 leader 多客户端共享会话。它们仍以 `#[ignore]` 保持普通本地 Cargo 套件轻量；CI 显式提供 `PAGER_BINARY` 并仅选择已登记的场景。采样失败后继续的真实 ACP 回归另外使用同次构建的 `GROW_BINARY` 和精确测试 runner。完成历史 replay 的多客户端用例在附加 viewer 前等待第一轮 `turn_completed` 持久化记录；另一用例在首轮仍流式输出时附加 viewer，验证 resident reconnect 不会把运行中的 turn 当作冷恢复中断。单项可按工作流中的完整 target/name 运行：

```sh
PAGER_BINARY="$PWD/target/debug/grow" cargo test --locked -p pager --test pty_e2e_minimal 'minimal::minimal_thinking_is_visually_distinct_from_output::minimal_thinking_is_visually_distinct_from_output' -- --ignored --exact --test-threads=1
```

Kitty keyboard 退出故障先检查 pager 的 reader 停止结果、writer 收束结果和
`kitty pop fence` 日志。正常退出在 raw mode 关闭前用 DA1 回复确认 pop 已被终端处理；
reader 仍存活、writer 卡住或查询失败时按原因降级。可运行
`cargo test --locked -p pager --lib kitty_fence` 与
`cargo test --locked -p pager-pty-harness --test kitty_pop_fence -- --ignored --nocapture`
复核决策及真实 PTY 顺序。行为契约见 [Kitty 终端恢复](../openspec/specs/client-surfaces/spec.md#requirement-kitty-keyboard-teardown-has-an-owned-reply-fence)。

原生 debug CLI 的会话线程为未优化 async 临时状态预留 32 MiB 栈；release 仍使用 8 MiB。该线程显式指定栈大小，不受测试运行器的 `RUST_MIN_STACK` 控制，不为调试栈开销改动生产调用链。

核心、跨平台协调及 Windows 存储回归统一设置 `RUST_MIN_STACK=16777216`，为调试测试夹具提供足够的测试线程栈。

Windows 协调清单用句柄级原子替换保留已有读者；独立会话加载使用共享读取目录能力，写者能力仍由独占 lease 管理。 Windows 存储回归还覆盖超过 MAX_PATH 的输入 artifact 和会话发布，以及扫描时排除普通文件而保留有效会话。对应契约见 [本机协调](../openspec/specs/local-coordination/spec.md) 与 [会话 Timeline](../openspec/specs/session-timeline/spec.md)。

会话 TTL 按已验证的 parent/child Timeline 闭链作为回收单位：所有成员必须分别过期、没有 writer 且结果与交付责任闭合。提交后的回收意图会在重启后继续完成；身份不符时保留整个意图和相关目录。显式单 session 删除遇到跨会话边时会拒绝并指出关联 ID。详见 [闭链回收 change](../openspec/changes/archive/2026-10-19-reclaim-closed-session-chains/design.md)。

询问按 source session 与 inquiry ID 建立稳定身份，source/target Timeline 约束重试、恢复、响应关联和取消；见 [询问身份与恢复契约](../openspec/specs/local-coordination/spec.md#requirement-inquiry-identity-and-replay-are-durable-across-session-recovery)。

source 的 durable startup 与 runtime 登记使用同一 admission 边界，查询也在该边界内回退到历史，避免把正在接纳的询问误判为重启遗留。对应契约见 [启动与查询一致性](../openspec/specs/local-coordination/spec.md#requirement-live-source-inquiry-startup-and-queries-share-an-admission-boundary)。协调回归包含 actor/runtime 单测及 `python3 scripts/test_local_coordination.py --binary <本次构建的 grow>` 的真实独立进程测试；包级测试通过不能代替后者。`Local agent coordination` workflow 对相关 PR、相关 main push 及手动触发执行；发布 workflow 仍对可原生运行的实际 release binary 执行同一进程回归。

模型 `ask_session` 的空白 inquiry_id 会按未提供处理并生成新 ID，非空值与外部协议 ID 继续严格校验；详见 [询问身份契约](../openspec/specs/local-coordination/spec.md#requirement-inquiry-scoped-operations)。

跨会话协调与 Windows 存储还应检查对应 `.github/workflows/` 的平台回归。OpenSpec CI 只做文档格式与归档完成状态检查，语义由场景、源码、测试和 review 共同核对。

用量状态栏由 `ChatStateEvent::SessionUsageUpdated` 投影到账本变化时的 transient `SessionInfoUpdate.meta["grow/sessionUsage"]`，复用 `PromptUsage`，不增加周期查询或模型输入。主模型 attempt、子 Agent 终态结算、session incomplete 与冷恢复边界写入 Timeline；新 actor 按结算身份恢复 lifetime aggregate，resident reconnect 不新增分段。Pager 按累计值替换、丢弃倒退及历史 replay；normal 状态栏只显示 lifetime 总体，`/usage` 另外按 Initial run、Resume #N 展示分段。计费窗口和点击行为见 [会话用量契约](../openspec/specs/client-surfaces/spec.md#requirement-ordinary-agent-status-shows-session-usage)。

Goal 状态栏、详情与只读回放的缓存率只用已分类的 cache hit + miss 输入作分母；未分类、历史聚合与未知消费使可计算比例带 `measured` 限定，无已分类输入则显示 N/A。其他无 Goal Behavior 沿用普通 session 用量投影。契约见 [Goal 缓存率](../openspec/specs/client-surfaces/spec.md#requirement-goal-usage-surfaces-show-measured-cache-hit-rates)。

Goal 完成工具要求 `completion_report` 写明原目标的达成结果、具体依据、验证及剩余限制。报告原文随 Complete 状态在同一 Control 中确认，失败回滚；迟到用量保留报告，编辑重激活清除当前完成依据。状态栏 `Goal: Done` 的既有点击入口打开完整报告，支持 Markdown 和滚动；恢复与只读回放使用相同原文，历史缺失时明确显示未归档。契约见 [Goal 完成报告](../openspec/specs/behavior-goal/spec.md#requirement-goal-completion-archives-its-report-with-the-terminal-state) 与 [Done 详情](../openspec/specs/client-surfaces/spec.md#requirement-done-goal-details-expose-the-archived-completion-report)。

Chat 流在 finish reason 与完整 usage 都已解析后，若用户 steer 或逻辑期限在可选尾帧等待期间取消 attempt，Sampler 仍以原 attempt 身份结算这份已确认的 token、cache 字段可用性与已报告费用；取消不等待尚未到达的 usage。未收到可信完整 usage 的已开始 attempt 继续按未知消费结算。状态栏在总消费不完整时保留 `≥`，只按 read 已知的输入样本计算缓存率，并以 `measured cache` 限定部分样本；`/usage` 的 coverage 是这些样本占全部已记录输入的比例，不包括未知消费。契约见 [中断用量结算](../openspec/specs/model-sampling/spec.md#requirement-cancellation-preserves-confirmed-attempt-usage) 与 [状态栏用量](../openspec/specs/client-surfaces/spec.md#requirement-ordinary-agent-status-shows-session-usage)。

`UsageLedger` 同时保留账本所属 Agent 与每个已完成子 Agent 的身份分项；子 Agent 以稳定 `subagent_id` 沿既有终态 ACK fold 进入父账本并持久结算。`PromptUsage` 当前只投影包含全部 Agent 的总体和 provider/model 分项，不把 Agent 分项暴露到 `/usage`、headless 或 normal 状态栏。

Pager 的 cancel notification 没有响应体；`TurnCancelling` 因此在短窗口后核对原 prompt 或 child lifecycle。只有匹配身份的持久 terminal 才结束任务；Unknown/error 展示取消未确认并保留目标，Running/Settling 继续有界核对。首次 session load 使用带来源 surface 的临时 Agent：失败会删除临时页并返回 Welcome、原 Agent 或 Dashboard，原地 reload 仍走既有事务回滚。

### Runtime 状态与恢复诊断

正常退出由默认全局 `Ctrl+D` 在一秒内连续按两次确认，再经 `grow/session/suspend_for_exit` 请求一个五秒共享期限的树挂起；首次按键不改变草稿、队列、当前回合或 Goal，UI 只有收到挂起确认才退出。挂起期间重复退出键合并到原操作，不强退。`Ctrl+C` 保留 composer 清理、局部控件操作和当前回合取消；取消中的重复 `Ctrl+C` 只重试取消，不关联退出。更新重启仍走同一树挂起屏障。排查恢复先核对 root 的 `TreeSuspended`、各节点 checkpoint、命令结算与物理 writer 退役，再检查 `RestorePrepared` / `Resumed` / `ExecutionReleased`。不要把 sampler 的部分输出、调用方超时或新的 actor 实例当作执行资格；中断或未知命令先核对其记录，不能自动重放。行为见 [session-timeline](../openspec/specs/session-timeline/spec.md) 与 [已确认退出契约](../openspec/specs/client-surfaces/spec.md#requirement-confirmed-quit-requests-bounded-session-tree-suspension)，验证见 [挂起记录](../openspec/changes/archive/2026-10-19-suspend-session-tree/verification.md)。

退出未确认后再次连续两次 `Ctrl+D` 走重试，Pending 期间重复退出仍合并；`r` 重试、`f` 显式强退与旧响应身份校验保留。normal/minimal、Welcome 和 Dashboard 共用可见的英文退出状态，长错误不能挤掉恢复动作行。见 [未确认退出交互](../openspec/specs/client-surfaces/spec.md#requirement-confirmed-quit-requests-bounded-session-tree-suspension)。

挂起接纳后立即关闭有限辅助活动准入，但 actor 邮箱继续处理既有 owner 的结算、checkpoint 查询和退役。迟到的 AI 补全/下一步建议返回无建议，其他有限命令沿既有关闭通道结束，不能在准入关闭后启动新模型请求或修改配置。五秒调用期限不取消尚未排空的物理 owner；未确认时保留原操作身份重试，不能伪造 checkpoint。该边界复用活动 tracker，见 [辅助活动挂起边界](../openspec/specs/model-sampling/spec.md#requirement-suspension-rejects-new-auxiliary-command-work-while-draining-admitted-owners)；交错回归见 [挂起准入验证](../openspec/changes/archive/2026-10-19-reject-auxiliary-commands-during-suspension/verification.md)。

挂起性能排查可按 `session suspension progress` 的 started、sampler_settled、commands_settled、draining_services、draining_auxiliary_owners、owners_drained、checkpoint_committed 阶段与 elapsed_ms 核对节点，再看整树确认日志。root 身份及已退役节点状态查询不再复制用不到的完整历史；需要 child 事实的快照仍完整验证，不能以缓存跳过校验。测量范围见 [挂起观察成本](../openspec/changes/archive/2026-10-19-streamline-suspension-runtime-observation/verification.md)。

若 Timeline 停在 `CommandsSettled` 且有后台命令，先核对其 `TaskCompleted` 通知是否以终态 `Ordinal` source version 接纳，再查节点 checkpoint；通知身份不合法会使挂起保持未确认。契约见 [后台命令挂起通知](../openspec/specs/session-timeline/spec.md#requirement-suspension-acknowledges-a-durable-tree-boundary)。 正常完成回执若已接纳（包括已消费），挂起复用其原正文与身份，不再次提交结算 JSON；见 [完成回执重试](../openspec/specs/session-timeline/spec.md#requirement-suspension-retries-reuse-canonical-completion-receipts)。挂起超时后，人工等待从当前 `SuspendRequested` 边界的 `Interaction` 事实重建，不能从已清空的运行时 registry 推断结果未知；仅本次挂起的 `Suspended` 保留等待，真实已解决的交互和之前操作的旧等待不复活。见 [人工等待保全](../openspec/specs/session-timeline/spec.md#requirement-suspension-retries-retain-human-wait-evidence)。

任务计划通过 SessionActor 的 `PlanningControl` 串行更新，检查 `Planning.previous` 和具体 source/result 引用。父项有效非完成修改要求 child 自行重规划，Consumed 不等于 Handled；普通完成和 Goal 还需核对必需交付的验收/解除记录。Plan 延期审阅使用独立 retained candidate 和不可变 decision identity；重试同一决定时 action、feedback 和候选都必须相同。契约见 [task-planning](../openspec/specs/task-planning/spec.md) 与 [behavior-goal](../openspec/specs/behavior-goal/spec.md)，实现记录见 [局部计划](../openspec/changes/archive/2026-10-19-evolve-task-planning/verification.md) 和 [Plan 审阅](../openspec/changes/archive/2026-10-19-retain-plan-for-later-review/verification.md)。
启动准入前失败的子任务以 `dispatch_failure` 保留未派发事实，`terminal: false` 不代表仍可处理父项更新。后续父 TODO 修改记录为带失败原因的 `Unapplied`；恢复投影前依据同一事实闭合遗留 replan，不生成子任务结果。必需交付仍须显式 Released。完成检查提示一次后，若下一次完成尝试的具体阻塞完全相同，返回 `planning_completion_blocked` 并停止本轮及 Agent completion recovery；Goal 内部回合沿原错误路径暂停，新的用户回合仍可处理义务。契约见 [未派发关联闭合](../openspec/specs/task-planning/spec.md#requirement-dispatch-failures-close-child-replanning-responsibilities) 与 [无进展完成检查](../openspec/specs/task-planning/spec.md#requirement-unchanged-completion-blockers-stop-automatic-sampling)。


Pager 的 prompt 与 input-admission 状态 RPC 共用 4 个工作许可和最多 8 个目标登记；watchdog 每轮最多派发 4 个查询，暂未获配额的目标在 250 ms 后重新核对。五秒期限包含许可等待；调用方超时后，仍在运行的 RPC 保留许可与目标登记，重连后的同目标查询不会绕过旧请求所有权。

状态核对从 Shell 的 `grow/queue/prompt_status`、`grow/input/admission_status` 和 Tasks backend 查询开始，再看 Pager 打开的子页面及提示状态。状态读取区分 runtime 缺失、持久会话可读、不可达、超时和证据无效；5 秒是控制查询的默认总期限，含排队与加载，调用方更早的 deadline 优先。超时只结束调用方等待；仍在执行的文件读取继续占用 worker 许可。查询失败不刷新最后确认状态或工作进展时间。取消状态未知时可以继续编辑和查询，但不能把它视为取消完成、推进队列或启动替代执行。实现与契约见 [observe-session-runtime-health](../openspec/changes/archive/2026-10-08-observe-session-runtime-health/proposal.md)、[runtime observation](../openspec/specs/session-timeline/spec.md#requirement-runtime-observations-preserve-query-uncertainty-and-ownership) 与 [取消核对](../openspec/specs/client-surfaces/spec.md#requirement-cancelling-turns-reconcile-against-authoritative-prompt-state)。

子 Agent 收尾受阻时，先查 Tasks 的执行 outcome/finalization 状态，再查 child Timeline、结果 artifact、父会话 Ended/usage 状态和 completion receipt。执行返回、child result、parent terminal、receipt 和交付分别确认；已有合法提交前缀必须 exact reconciliation，ACK 不明时核对原 identity/payload，不能重跑 provider 或工具。正常收尾把完整推进链放在共享 30 秒 deadline 内；后续恢复周期也共用固定的 30 秒绝对期限，查询和重连不重置期限，仅合法新准入屏障可开启下一周期。receipt 单次观察最多 5 秒、最多三次可重试尝试且复用同一总预算。等待结束而底层 I/O 仍在运行时，in-flight 标记和 writer lease 保留，不能派发重复写入或开启替代 writer；只有旧 I/O/owner 实际结束、完整 drain 并释放 lease 后，替代 writer 才能校验和 sync 恢复。panic 路径把 child handle、thread 和输出写入 in-flight 标记交给 Coordinator 持有的恢复责任，usage 保持 incomplete，不伪造输出或终态。内存中最多保留 64 项待收尾责任及合计 8 MiB 的原始输出；超额正文标为不可用，不生成替代内容。纯内存正文在永久不可写且进程退出时无法保证恢复。见 [recover-subagent-terminal-commit](../openspec/changes/archive/2026-10-08-recover-subagent-terminal-commit/proposal.md)、[收尾责任](../openspec/specs/session-timeline/spec.md#requirement-subagent-finalization-retains-one-accountable-owner)、[exact replay barrier](../openspec/specs/session-timeline/spec.md#requirement-exact-replay-commits-acknowledge-durable-barriers) 和 [取消结算边界](../openspec/specs/model-sampling/spec.md#requirement-subagent-cancellation-settles-admitted-attempts-before-closing-the-child-timeline)。

输入发送 ACK 丢失时，Pager 保留原 session/prompt/input identity、请求摘要和可用快照；状态只读未发现输入不等于明确未接纳。未编辑重试必须复用原 identity 和精确快照；编辑后的内容只有用户显式发送才成为新意图。提交 Hook 中断按已有执行证据恢复，不自动重跑不明副作用。确认影响共享 Grow 存储域的满盘/配额故障后，该管理实例停止该域新的 prompt/child/provider/tool 准入；单 session 完整性或权限故障只关闭相应 owner，诊断、取消和合法恢复保留可用。自动清理基于已验证 Timeline、输入、子任务/回执和 artifact 引用；candidate scan 不是删除许可，删除时还须在稳定顺序取得相关 writer lease 并复核。独立 workspace GC 缺乏 session authority 时跳过关联任务资源；已修改工作树只有取得合法 snapshot 和引用保护后才能回收。TTL 发现最多检查 4,096 个候选及 8,192 个目录项；一次有界 Timeline 校验最多读取 16 MiB、50,000 个事件和 4,096 个引用，依赖扫描超过 4,096 个 child spawn 即停止并保留候选。扫描超预算、损坏或无法读取时保守保留并报告原因。存储不可写同时进程退出时，未持久化正文仍可能丢失；保留限制不是零损失承诺。见 [preserve-uncertain-input-and-recovery-resources](../openspec/changes/archive/2026-10-08-preserve-uncertain-input-and-recovery-resources/proposal.md)、[输入重试身份](../openspec/specs/input-admission/spec.md#requirement-user-submission-retries-retain-one-durable-input-identity)、[恢复资源清理](../openspec/specs/session-timeline/spec.md#requirement-destructive-maintenance-revalidates-recovery-references-at-deletion) 和 [已验证关闭会话清理](../openspec/specs/session-timeline/spec.md#requirement-completed-session-cleanup-is-independent-of-live-predecessors)。

本地草稿继续使用现有单源格式和 256 KiB 记录预算，不持久化图片。普通和 steer 的显式图片输入首次发送直接使用现有 RAM-only 请求保留机制，经身份校验进入 ACP，不要求先保存到文本草稿；`input_intent.memory_only.*` 与磁盘 checkpoint 日志分开。服务端接纳前退出进程仍可能丢失内存图片。无图片请求的超预算或真实存储错误仍保留快照并提示未保存，等待用户显式重试。已持久接纳的图片由 Shell 的输入 payload 与附件引用恢复，不能用编辑后的正文重构原请求。见 [显式图片首次发送](../openspec/specs/client-surfaces/spec.md#requirement-explicit-image-submissions-do-not-require-text-only-draft-persistence)。

发送前 checkpoint 失败会释放同一请求的本地 Sending 占位及 watchdog，保留原身份和图片供显式重试；不会关闭已运行或其他请求的前台。排查时先核对 `input_intent.checkpoint.complete` 与 `input_intent.ready.execute`，区分尚未派发的本地失败与远端接纳未知。见 [本地 checkpoint 失败收尾](../openspec/specs/client-surfaces/spec.md#requirement-failed-local-input-checkpoints-release-their-submitting-placeholder)。

## 债务与历史

无关审计发现登记 [OpenSpec backlog](../openspec/backlog.md)，明确条件、影响、证据和未来验收；等待单独启动。既有过程文档见 [历史登记](../openspec/baseline.md#历史资料)，不延续其中的任务清单。

托管配置语法检查器的退出与清理契约见 [configuration-rules](../openspec/specs/configuration-rules/spec.md#requirement-managed-syntax-validator-exit-cleanup)；实现位于 `config/src/managed_text/validator.rs`。

托管配置的回滚冲突与恢复材料边界见 [configuration-rules](../openspec/specs/configuration-rules/spec.md#requirement-managed-config-rollback-preserves-observed-conflicts)；文件锁与检查不能替代非协作编辑器之间的原子比较交换。

托管配置实际读取量限制见 [configuration-rules](../openspec/specs/configuration-rules/spec.md#requirement-managed-config-reads-obey-byte-budgets)，元数据预检不代替读取预算。

托管配置源快照的同句柄约束见 [configuration-rules](../openspec/specs/configuration-rules/spec.md#requirement-managed-source-fields-share-a-file-handle)，它不等于对原地并发写入的原子快照。

托管配置规划必须保持源可读性，见 [configuration-rules](../openspec/specs/configuration-rules/spec.md#requirement-managed-plans-preserve-source-readability)；最终大小包含原文、标记和条目正文。

托管配置批量渲染复用原文解析计算条目状态和替换范围，保留完整输出校验。性能探针和前后数据见 [batch-managed-config-render](../openspec/changes/archive/2026-09-08-batch-managed-config-render/verification.md)。

图片占位符是文字锚点，不授权按其中的路径读取文件；进入模型上下文前会去掉路径，图片字节只从显式附件接纳。契约见 [client-surfaces](../openspec/specs/client-surfaces/spec.md#requirement-textual-image-placeholders-do-not-load-files)。

图片附件URI使用 `client-support::placeholder_images::file_uri_from_path`，不能直接拼接路径字符串。生成与解析边界见 [图片URI契约](../openspec/specs/client-surfaces/spec.md#requirement-image-file-uris-preserve-literal-path-bytes)；占位符在客户端展示时仍可使用原始文件路径。

Dashboard 的路径粘贴共用 `insert_dropped_paths`，保留图片和普通文件的顺序；不要在插入前使用图片专用过滤器。见 [混合路径契约](../openspec/specs/client-surfaces/spec.md#requirement-dashboard-preserves-mixed-drop-paths)。问题模式和异步目标有效性由调用入口检查。

终端输入批次中，已完成的括号粘贴保留独立边界，其后的普通按键逐个路由；无括号粘贴检测只处理此前的按键段。文件补全查询更新会立即清空旧结果，`@` 上下文即使暂时无可见结果，Esc 仍可关闭它和目录钻取锚点。见 [粘贴边界](../openspec/specs/client-surfaces/spec.md#requirement-paste-collection-preserves-input-batch-boundaries) 与 [文件搜索当前查询](../openspec/specs/client-surfaces/spec.md#requirement-file-search-results-belong-to-the-current-query)。

异步文件 URL 和原始剪贴板文本是两个独立来源。分类未命中时，通过 `ClipboardPasteSource::file_url_text_on_miss` 判断是否需要保留 URL 文本；仅在成功探测且没有原始非空文本时使用。见 [异步 URL 回退契约](../openspec/specs/client-surfaces/spec.md#requirement-deferred-file-urls-survive-classification-miss)。

macOS `pbpaste -Prefer txt` 与图片 AppleScript 使用有界进程运行器：5 秒执行期限、stdout/stderr 各1 MiB，并回收所属进程组。图片 AppleScript 子进程继承单文件 50,000,000 字节 `RLIMIT_FSIZE`；图片写入私有临时文件后，Grow 最多读取 50,000,001 字节并拒绝超限结果。子进程文件限制不约束 helper/AppKit RSS、多个文件的合计用量或图片解码内存。简单图片粘贴会增加约 0.5–0.9 秒子进程与文件往返延迟。AppKit 在 Grow 内只用于读取 `changeCount`/`types` 元数据。见 [剪贴板图片读取契约](../openspec/specs/client-surfaces/spec.md#requirement-macos-clipboard-encoded-images-have-a-read-budget)、[子进程生命周期契约](../openspec/specs/client-surfaces/spec.md#requirement-clipboard-image-scripts-have-bounded-process-lifetimes) 与 [transfer file 写入预算](../openspec/changes/archive/2026-09-24-bound-macos-clipboard-transfer-file/specs/client-surfaces/spec.md)。

Kitty 非 PNG 转换在调用 sips 或 Rust 解码前检查源像素预算，见 [转换像素契约](../openspec/specs/client-surfaces/spec.md#requirement-kitty-image-conversion-bounds-source-pixels)。编码数据大小、源像素数、转换进程生命周期和终端直接解码是不同的资源边界。Grow 构造的 Kitty/iTerm2 图片上传 escape buffer 另有 100,000,000 字节序列化上限，见 [终端图片输出预算 change](../openspec/changes/archive/2026-09-24-bound-terminal-image-escape-output/specs/client-surfaces/spec.md)；它不限制终端进程解码或缓存图像的内存。

sips 源文件和结果文件由单个私有临时目录持有，错误出口也通过目录所有权回收，见 [转换临时文件契约](../openspec/specs/client-surfaces/spec.md#requirement-sips-conversion-owns-private-temporary-files)。不要恢复为共享临时根目录中的手工命名文件和分散清理。

sips 通过 `run_sips_command` 在独立进程组中运行，执行期限为10秒，返回前处理进程组回收；见 [转换执行契约](../openspec/specs/client-surfaces/spec.md#requirement-sips-converter-has-an-owned-execution-deadline)。该期限不涵盖文件I/O或不可中断的内核等待。

sips 结果用同句柄检查及有界读取限制为100MB，见 [结果读取契约](../openspec/specs/client-surfaces/spec.md#requirement-sips-output-reads-have-an-encoded-budget)。这不限制转换器写入磁盘前的产物大小，也不覆盖 Rust 回退编码器。

sips 进程错误保留启动、进程组登记、等待或清理阶段及原始错误文本，见 [进程诊断契约](../openspec/specs/client-surfaces/spec.md#requirement-sips-process-failures-identify-their-stage)。不能仅凭 EPERM 推断是清理失败。

图片查看器的真实 Enter 入口只捕获内存/文件来源与终端协议，读取和转换由 `LoadImageViewer` 任务完成；每次打开使用独立 owner，见 [查看器加载契约](../openspec/specs/client-surfaces/spec.md#requirement-prompt-image-viewer-loading-is-deferred)。测试必须覆盖真实入口，不能仅手工构造 loading 状态来证明生产接线。

查看器后台加载在内存复制或文件读取时执行50MB源字节预算，见 [查看器来源预算](../openspec/specs/client-surfaces/spec.md#requirement-background-image-viewer-source-bytes-are-bounded)。它与拖入图片共用有界文件读取，不代替并发任务或转换内存预算。

Slash MRU 的进程内写线程不能协调其他 Grow 进程；每次快照使用目标目录中的独占临时文件再替换，见 [MRU 写入契约](../openspec/specs/client-surfaces/spec.md#requirement-slash-mru-writes-own-unique-temporary-files)。完整快照仍采用最后写入者覆盖，不等于跨进程历史合并。

Slash MRU 首次同步加载只接受普通文件，编码输入上限为1 MiB；拒绝读取时禁用本会话持久化以保护源文件，见 [MRU 加载契约](../openspec/specs/client-surfaces/spec.md#requirement-slash-mru-loading-bounds-encoded-input)。这不提供慢文件系统的读取超时。

Slash MRU 序列化快照复用1 MiB上限；超限时在后台交接和文件发布前拒绝，控制器保留dirty以便重试，见 [MRU 写入大小契约](../openspec/specs/client-surfaces/spec.md#requirement-slash-mru-snapshot-writes-obey-the-encoded-input-allowance)。该限制不约束快照序列化时的临时内存。

Slash MRU 后台仅保留一个待写最新完整快照，写入在锁外执行；线程不可用时保留dirty重试，不回退到UI同步写盘，见 [MRU 调度契约](../openspec/specs/client-surfaces/spec.md#requirement-slash-mru-background-writes-coalesce-pending-snapshots)。进程退出仍不保证flush。

历史搜索在提交时合并待处理items/query，用容量1通知唤醒匹配线程；提交和关闭不等待队列容量，见 [历史搜索调度契约](../openspec/specs/client-surfaces/spec.md#requirement-history-search-submission-does-not-wait-for-worker-capacity)。正在执行的匹配不会被强制中断。

历史搜索结果回传提交时的请求编号；UI仅接受当前请求，并在新查询/刷新时清除旧的可选结果，见 [历史结果契约](../openspec/specs/client-surfaces/spec.md#requirement-history-search-accepts-only-current-request-results)。

Pager `@` 文件搜索按提交的 query identity 接纳异步结果，并在新查询提交时先清空旧快照；tick generation 不能替代请求身份。见 [文件搜索结果归属](../openspec/specs/client-surfaces/spec.md#requirement-file-search-results-belong-to-the-current-query)。

本地草稿恢复检查打开句柄为普通文件，并在解析前检查实际读取是否超过256KiB；隔离前核对当前路径仍指向已读取的文件，避免把已观察到的路径替换项移入隔离目录。超限沿用隔离策略，见 [草稿读取契约](../openspec/specs/client-surfaces/spec.md#requirement-local-draft-recovery-bounds-source-reads)。Unix特殊文件打开不等待FIFO写入者。

本地草稿的composer与staged_prompt必须互斥；共享校验在空记录删除前执行，异常磁盘记录隔离而非选择其中一份恢复，见 [草稿互斥契约](../openspec/specs/client-surfaces/spec.md#requirement-local-draft-records-contain-at-most-one-prompt-source)。

Agent最后一个草稿所有者关闭后，检查点成功才释放干净运行时缓存；失败保留最新内容和重试期限，重开优先恢复它，见 [关闭草稿生命周期](../openspec/specs/client-surfaces/spec.md#requirement-closed-agent-drafts-release-only-recoverable-runtime-state)。磁盘草稿不会因释放缓存而删除。

本地草稿无效化失败保留逐键删除期限，关闭和会话绑定不会丢失意图；重开抑制旧内容恢复，新有效草稿取消该键旧删除，见 [草稿无效化契约](../openspec/specs/client-surfaces/spec.md#requirement-local-draft-invalidation-retries-without-restoring-stale-content)。意图只在内存中，持续I/O失败后进程退出不保证跨重启清理。

本地草稿的路径解析、读取、写入和删除由串行 worker 执行。Pager 只在事件循环中捕获草稿快照，并核对异步恢复结果是否仍属于当前会话和编辑状态；正常退出等待最新草稿检查点。行为见 [草稿 I/O 契约](../openspec/specs/client-surfaces/spec.md#requirement-local-draft-filesystem-latency-does-not-block-pager-interaction)。

CWD 草稿绑定到新 session 时，刚出队的 in-flight 输入也属于当前编辑所有权；空 composer 不能作为恢复旧草稿的充分条件。发送检查点保留原 prompt identity，只有确认成功才派发 ACP。绑定边界见 [草稿 I/O 契约](../openspec/specs/client-surfaces/spec.md#requirement-local-draft-filesystem-latency-does-not-block-pager-interaction)，运行证据见 [已归档修复](../openspec/changes/archive/2026-10-19-preserve-input-during-session-bind/verification.md)。选中的 Skill 同时保留原 name/path/plugin 来源，明确重试不重新选择同名来源，见 [统一 Skill 加载](../openspec/changes/archive/2026-10-19-unify-skill-loading/verification.md)。

/export路径补全最多消费1000个目录迭代结果，隐藏项及错误也计数，最终最多返回100个建议，见 [导出补全预算](../openspec/specs/client-surfaces/spec.md#requirement-export-path-completion-bounds-directory-enumeration)。单次文件系统调用延迟不受此数量预算保证。

滚动日志仅接受普通文件目标，打开句柄检查后才截断；Unix FIFO不等待reader，失败后使用已有禁用状态，见 [滚动日志目标契约](../openspec/specs/client-surfaces/spec.md#requirement-scroll-recorder-rejects-special-file-targets)。普通显式文件覆盖和符号链接行为保留。

单个滚动日志记录器最多接受64MiB（含换行），按完整行停止、flush并报告关闭，不自动轮转或删除旧文件，见 [滚动日志字节预算](../openspec/specs/client-surfaces/spec.md#requirement-scroll-recording-bounds-each-capture-by-complete-lines)。末尾手势可能尚未finalize，不能把停止当成完整实验结束。

输入诊断由实际处理按键的AgentView记录，委派子视图不再消费父textarea delta；导出解析子视图及Dashboard附着目标，见 [诊断目标契约](../openspec/specs/client-surfaces/spec.md#requirement-input-diagnostic-dumps-describe-the-input-owner)。Dashboard的Esc仍用于关闭popup，未增加快捷键入口。

Pager统一日志使用初始化时保存的ACP发送端和Tokio runtime，由单个 sender 顺序转发；普通线程可写入。初始化前保留受限缓冲，单条编码上限64 KiB，待发送队列最多256条或1 MiB，一次发送最多16条或256 KiB；超限新条目被丢弃。退出时 `flush_blocking` 等待调用前已接纳条目的发送结果，包括此前已出队的批次，总等待不超过2秒；发送结果不证明远端落盘。见[日志转发归属契约](../openspec/specs/client-surfaces/spec.md#requirement-pager-log-dispatch-preserves-initialized-ownership)与[退出等待契约](../openspec/specs/client-surfaces/spec.md#requirement-pager-log-flush-has-a-bounded-delivery-wait)。

退出时当前日志批次的确认等待最多2秒，避免诊断发送阻止终端恢复；超时只结束本地等待，已入队通知仍可能被处理，见[flush等待契约](../openspec/specs/client-surfaces/spec.md#requirement-pager-log-flush-has-a-bounded-delivery-wait)。

Recap扩展只在会话命令成功入队后确认接纳；命令接收端已关闭时返回ACP错误，手动请求沿既有错误路径清理等待提示，见[Recap接纳契约](../openspec/specs/client-surfaces/spec.md#requirement-recap-admission-reflects-command-enqueue)。配置默认开启，可由远端设置、配置或环境变量关闭。

Pager读取recap扩展响应中的接纳结果；disabled、拒绝或无效响应会结束对应会话的手动等待提示，自动请求失败保持安静，见[Recap响应消费契约](../openspec/specs/client-surfaces/spec.md#requirement-pager-consumes-recap-admission-outcomes)。单次禁用响应不会永久改写连接能力。

回放中的recap只恢复历史展示，不结束当前手动recap等待，也不消耗本次离开期间的自动资格，见[Recap回放契约](../openspec/specs/client-surfaces/spec.md#requirement-replayed-recaps-do-not-settle-current-feedback)。

自动recap的展示记录和重试退避按SessionId隔离；轮询与返回焦点均查询当前根会话。Pager在每个新离开周期生成独立身份并随自动请求传给Shell，Shell在成功通知中回传；Pager只展示属于当前周期的实时自动结果，回放与手动recap不受此身份过滤。契约见[自动recap归属](../openspec/specs/client-surfaces/spec.md#requirement-automatic-recap-bookkeeping-is-session-scoped)。

自动recap在连接重建或会话重载期间不派发，也不记录轮询退避；若此时返回焦点，本次离开周期的自动机会按尽力语义丢弃。重连完成后沿用新shell发布的能力，后续手动请求和新的离开周期恢复正常，见[重连期间自动recap契约](../openspec/specs/client-surfaces/spec.md#requirement-automatic-recap-is-suppressed-during-reconnect)。

公告隐藏状态通过同目录独占临时文件写入、sync后原子替换，失败沿pager持久化结果传递，不再恒报成功，见[公告提交契约](../openspec/specs/client-surfaces/spec.md#requirement-hidden-announcement-state-commits-complete-snapshots)。这不提供跨进程合并或异步请求顺序保证。

共享atomic state writer仅在成功独占创建临时文件后执行失败清理，名称碰撞不会删除已有临时文件，见[临时文件归属契约](../openspec/specs/configuration-rules/spec.md#requirement-atomic-state-writers-preserve-unowned-temporary-paths)。

公告隐藏状态读写共用1 MiB编码字节上限；读取只接受普通文件，超限或非法输入按全部可见处理且不改源文件，见[公告IO边界契约](../openspec/specs/client-surfaces/spec.md#requirement-hidden-announcement-state-has-bounded-io-admission)。Unix FIFO非阻塞拒绝不等于普通慢盘读取有总时限。

公告偏好在单个AppView内最多一个写入任务；隐藏、显示和更新清理在等待期间合并为最新集合，完成后再提交，见[公告写入顺序契约](../openspec/specs/client-surfaces/spec.md#requirement-announcement-preference-writes-follow-local-change-order)。跨进程合并和退出flush不在此保证内。

认证 helper 的失败诊断位于 `shell/src/auth/token_output.rs` 和 `auth_provider.rs`。排查时使用退出状态、JSON 类别/行列及捕获字节数；不要把 helper 原始输出重新拼进日志。见 [凭据输出诊断契约](../openspec/specs/configuration-rules/spec.md#requirement-credential-helper-failures-do-not-echo-output-payloads)。

认证 helper 的相对 cwd 以 Grow 进程工作目录为基准解析一次，再用于程序路径和子进程工作目录。见 [helper 路径契约](../openspec/specs/configuration-rules/spec.md#requirement-relative-credential-helper-cwd-resolves-once)。

认证 helper 的提前刷新窗口与发送有效期分别判断；短效 token 在实际过期前可发送，刷新失败会撤下旧值。实现入口为 `auth_provider.rs::cached_token` / `ensure_fresh_token`，见 [短效凭据契约](../openspec/specs/configuration-rules/spec.md#requirement-valid-short-lived-helper-tokens-remain-sendable)。

采样认证排障使用 auth_type、auth_scheme 与认证头 presence 字段；client_post 和 sampling_request 不记录凭据前后缀。见 [采样认证日志契约](../openspec/specs/model-sampling/spec.md#requirement-sampling-authentication-logs-omit-credential-fragments)。401 attribution 回调为独立路径。

`grow trace` 的 CLI 分发直接进入会话快照导出，不要求模型配置能够成功解析。会话缺失和输出失败仍按原路径报告，见 [Trace 配置独立性契约](../openspec/specs/client-surfaces/spec.md#requirement-trace-export-does-not-require-valid-model-configuration)。

Prompt 中的 ACP 图片附件与从 query 文本提取的 base64 图片都经过 `normalize_images_with_notices`。每批的压缩、丢弃及保留原图 fallback 说明进入当前 prompt context，并使用现有图片 Session 通知展示；权限文本仍来自清理后的 query。两类来源各自形成 normalization 批次，索引不跨批次合并。每批先限 25 图、80 MB base64 payload，进程内待处理批次合计限 160 MB；超额图片以原始索引丢弃。完整像素解码限 50 MP，不改变历史图片的 provider 有效性上限。输入 base64 decode、可选转码与 PNG 编码、compute 和输出编码共用一个进程级 worker permit；取消的请求不能提前释放仍在运行的 worker 或其输入字节预留。契约见 [内嵌图片处理结果](../openspec/specs/input-admission/spec.md#requirement-inline-query-images-retain-normalization-outcomes)、[图片正规化 worker 准入](../openspec/specs/client-surfaces/spec.md#requirement-image-normalization-workers-have-process-wide-admission) 与 [图片正规化预算](../openspec/specs/model-sampling/spec.md#requirement-image-normalization-uses-the-bounded-compute-path)。

MCP `tools/call` 的一次请求由 `mcp/src/servers.rs::call_tool_cancel_aware` 持有原 peer/request id 和同一绝对期限；取得 id 后的超时、turn 取消只由该调用守卫 best-effort 发一次 `notifications/cancelled`，不向同名替代服务发送，超时工具也不重放。当前 ACP reverse bridge 不转发无 id 通知，因此端到端保证范围是 stdio/Streamable HTTP。排障时以原请求 id 和 transport episode 对照，不能把通知成功等同于远端副作用撤销。契约见 [MCP 取消](../openspec/specs/extension-runtime/spec.md#requirement-abandoned-mcp-calls-notify-their-originating-service)。

普通 MCP 工具调用遇到传输丢失、发送错误或 HTTP MCP 错误时，不会自动重发原 `tools/call`；传输错误和超时会明确报告结果未知。符合恢复条件的旧连接可被重置，供下一次独立调用重新初始化；HTTP MCP 错误本身仍按服务返回的错误呈现。实现与验证见[防止不确定 MCP 工具调用重放 change](../openspec/changes/archive/2026-10-19-prevent-uncertain-mcp-tool-replay/verification.md)。

Pager 的进程内 ACP session 会通过配置 watcher 刷新活动 session 的 MCP catalog，项目目录监听受 `mcp_recursive_config_watch` 控制；见 [活动会话 MCP catalog 刷新](../openspec/specs/extension-runtime/spec.md#requirement-live-acp-sessions-refresh-their-mcp-catalog-from-configuration-changes)。 监听目录沿用配置合并的 cwd 到仓库根目录链，共享祖先监听按注册目录保留。删除或替换会立即清理工具注册和搜索快照，迟到的旧连接结果受 client episode 约束；启用事件 dispatcher 时，重连 Ready 和 tools/list_changed 会重新发现完整目录（包括空列表），不依赖自动重连开关。冷恢复使用当前磁盘配置和本次 session/load 的客户端目录；见 [MCP catalog 生命周期](../openspec/specs/extension-runtime/spec.md#requirement-mcp-catalog-lifecycle-follows-the-effective-configuration)。

MCP doctor、工具注册和 descriptor 共用完整 `tools/list` 分页收集；后续页失败或 cursor 循环会整体报错。后台发现结束后才取得当前 Agent 的发布目标，注册和搜索快照与 Agent 切换在同一本地边界内完成；网络等待不阻塞切换。排查 `search_tool` 可见但 `use_tool` 未注册时，应检查当前会话目录与切换时序，不能仅凭未注册错误认定服务断线。见 [MCP 目录发现与发布](../openspec/specs/extension-runtime/spec.md#requirement-mcp-initialization-publishes-to-the-current-agent-bridge)。

MCP 的服务 `name` 同时作为 catalog、客户端和工具前缀身份。跨来源同名项按配置高于插件、客户端高于配置的既有优先级选取一项；单个 ACP 客户端数组内重名会在 new/load/update 接纳前报错。显式 `name + serverUrl` 必须匹配同一当前项，URL 不作跨名称别名；见 [服务身份](../openspec/specs/extension-runtime/spec.md#requirement-mcp-server-name-is-the-session-identity)。

辅助策略与连接配置独立刷新：修改 `disabled_mcp_tools`、启动/工具超时或 per-tool timeout 不需要重连未变化的 endpoint。可信项目按 server name 覆写全局 mask，空数组解除继承的禁用；工具禁用同步作用于模型注册、搜索快照和 direct call。删除超时覆写恢复下一层或默认值，`_meta` 仍优先；已准入调用保留自己的期限。见 [辅助策略热刷新](../openspec/specs/extension-runtime/spec.md#requirement-live-mcp-auxiliary-policy-follows-effective-configuration)。

MCP 工具的模型路径把 `structuredContent` 与 text/resource 描述投成受既有文本限额约束的正文，image block 与 image resource 则作为运行时附件绕开文本截断，并在 Shell 结果结算时走原有正规化、图片预算与 Timeline follow-up。`use_tool` 的三层 JSON value 往返用 `TypedToolOutput.model_output` 携带附件，再还原到运行时 `MCPOutput`；默认 ACP `raw_output` 不序列化这些图片的 base64。显式 `expose_image_base64` 仍会把有界原始数据写进正文，长正文可能被截断并保存完整文件。direct `grow/mcp/call` 不写模型 Timeline，返回原生完整 `CallToolResult`；其 JSON 响应载荷上限为 16 MiB，超限报错而不返回残缺 JSON。契约见 [MCP 结果完整性](../openspec/specs/extension-runtime/spec.md#requirement-mcp-tool-results-retain-structured-and-typed-content) 和 [附件恢复](../openspec/specs/session-timeline/spec.md#requirement-mcp-image-evidence-survives-text-output-truncation)。

已批准的闲置接口清理边界以 [配置规则](../openspec/specs/configuration-rules/spec.md)、[工具协议](../openspec/specs/tool-authorization/spec.md)、[技能运行时](../openspec/specs/extension-runtime/spec.md) 和 [客户端设置持久化](../openspec/specs/client-surfaces/spec.md) 为准。技能列表由 SkillManager 管理；配置整份写入入口仍保留。

显式技能加载统一使用已注册的 `load_skill(name, args)`：用户选中的技能由 Runtime 直接派发同一工具，模型调用走普通工具准入；初始化 `skills:` 仍保留预加载角色，普通 `read_file(SKILL.md)` 仍是文件读取。正文准备复用 `tools/src/implementations/skills/skill.rs` 的严格 UTF-8 读取、链接处理与单次字面替换；输出先做 checked-length 预检，单项及同一用户组均受 1 MiB 上限约束，参数后缀也计入，取消会在准备期间检查。用户组在一个 `Input::Consumed` 中原子发布原文和有序 `SkillLoadUser`，不会把正文拼回原 prompt；模型路径以一个配对 `ToolResult` 和 `SkillLoadModel` 发布。提交 ACK 未确认时结果属于未知状态：不能宣称 Loaded、继续依赖正文或按新身份重跑；必须按稳定身份向权威 Timeline 对账。首次关联请求获得 accepted response 前，Timeline 保护完整正文；首次 accepted response 消费这项保护，显式 TurnEnd 会结算仍未消费的正文。Runtime 挂起保留原 Turn，不等于 TurnEnd，也不解除正文保护。实现契约见 [extension-runtime](../openspec/specs/extension-runtime/spec.md) 与 [input-admission](../openspec/specs/input-admission/spec.md)，验收证据见 [unify-skill-loading](../openspec/changes/archive/2026-10-19-unify-skill-loading/verification.md)。

Skill discovery 在 Agent 构建时生成唯一 `SkillManager` 目录，模型目录只展示名称及简短描述，不提供安装路径或提前加载完整正文。使用技能必须调用 `load_skill(name, args)`；必要时再调用 `load_skill(name, resource="references/api.md")` 读取一个包内 UTF-8 资源，resource 不与 args 混用、不替换模板、不执行脚本。资源经过实际文件路径的授权、1 MiB 上限、取消和既有 durable SkillLoad 发布；加载后的资料保持原文与来源身份。名称先解析再判定资格，禁用、缺失或不可调用明确返回无法加载，不通过同名条目、Read、Shell 或另一副本绕过。Agent 的 `skills:` 初始化依赖无法满足时明确构建失败，已预加载技能仍保留目录身份以读取后续资料。普通文件编辑与审阅仍不等于技能激活。契约见 [按需资源加载](../openspec/specs/extension-runtime/spec.md#requirement-skill-resources-load-progressively-through-the-same-tool) 与 [初始化依赖](../openspec/specs/configuration-rules/spec.md#requirement-agent-skill-preloading-respects-disabled-entries)。

视觉辅助请求只有图片和固定英文识图 prompt，不带图片所在消息的文字、当前任务、Skill 或会话历史；Sideband 来源引用只用于本地证据与恢复。新 prompt 的缓存身份与已完成 Sideband 恢复一致，不复用旧 prompt 的结果。识图与 OCR 结果保留原语言。只有存在待处理图片时才创建普通操作行；视觉调用、OCR、转换数量或失败在同一行更新，固定文案使用英语。该行绑定 prompt，复用回合终态的身份校验；活跃重连保留动效，离线截点缺少终态时标记 incomplete。见 [视觉辅助契约](../openspec/specs/model-sampling/spec.md) 与 [图片操作行](../openspec/specs/client-surfaces/spec.md#requirement-image-recovery-is-one-visible-operation-row)。

图片工具与关联 response carrier 的内部投影不生成 assistant 技术占位正文；派生模型请求（含直接 Chat 压缩请求）和子 Agent 背景清理两种历史完整保留值。normal/minimal/子 Agent 的共用 assistant 块在流式起始前缀尚无法判别时短暂缓存，精确占位消息不进入展示、搜索和复制，普通解释原样释放。原始 Timeline 与 provider evidence 保留，历史正文由同一 admission 派生，`Converted 1 image` 等操作行继续使用原有入口。见 [模型投影契约](../openspec/specs/model-sampling/spec.md#requirement-internal-image-projection-status-is-not-assistant-speech) 和 [TUI 占位过滤](../openspec/specs/client-surfaces/spec.md#requirement-tui-omits-complete-internal-image-projection-placeholders)。

发布门禁通过 reusable workflow 接收 release validate 得到的完整 commit SHA，checkout 后核对，core、三平台协调/导出、Windows storage、OpenSpec 任一失败、跳过或取消均阻止发布。精确测试使用 `scripts/run_exact_cargo_test.py`，先确认唯一名称，再确认该测试实际通过且无 ignored，防止重命名后 0 test 假通过；本地运行示例：`python3 scripts/run_exact_cargo_test.py --package pager --lib trajectory_cmd::tests::transcript_download_http_uses_production_export_archive`。见 [发布门禁](../openspec/specs/development-workflow/spec.md#requirement-release-regression-gates-are-bound-to-the-published-source)。

精确测试 runner 把编译、枚举和执行分开报告，并实时转发输出；默认期限依次为 60 分钟、60 秒、15 分钟，可用 `--compile-timeout`、`--list-timeout`、`--test-timeout` 覆写，必须为有限正数。此预算保留既有 90 分钟 job 总门禁，不代表冷构建基准。超时或取消会尝试终止 Unix 进程组／Windows 子进程树并回收直接子进程；输出读取和收尾也有界，无法确认后代结束时明确失败，不声称已回收。读异常不会当作正常 EOF；零测试、缺失名称和未显式启用的 ignored 测试继续失败。正常流式输出用握手回归验证，超时、取消、SIGTERM 和父退出后仍被后代持有的管道各有失败路径用例；现有 Windows storage gate 会执行跨平台用例，本机验证不代替 Windows 原生执行。见 [精确 runner 契约](../openspec/specs/development-workflow/spec.md#requirement-exact-cargo-regression-runners-expose-bounded-phases)。

官方构建另外把已校验的 `RELEASE_COMMIT` 传入所有容器：CLI build script 要求完整 40 位十六进制 SHA，以前七位小写生成版本标识，并跟踪环境变化使 Cargo 缓存失效。native 与 OHOS smoke 比较完整 `grow <version> (<commit>)`，拒绝 `unknown` 或错误提交号；OHOS 显式提供 SHA 时也必须提供 `GROW_VERSION`。不依赖容器 Git 所有权，也不修改 Git 全局信任规则。见 [构建身份修复与验证](../openspec/changes/archive/2026-10-20-bind-release-build-identity/verification.md)。cross 0.2.5 使用固定 SHA-256 的官方 Linux x86_64 二进制包，避免源码安装的独立旧 lock 引用已撤回依赖；现有 cross runner 均为 x86_64。Core 门禁对实际 vendored sqlite-vec 执行严格 C 编译及完整 SQL/生命周期 sanitizer 检查（ASan、UBSan 与泄漏检测），原始诊断证据见 [第三方诊断修复](../openspec/changes/archive/2026-10-20-repair-third-party-build-diagnostics/verification.md)。

metadata 名称释放的后续修复与验证见 [所有权修复](../openspec/changes/archive/2026-10-20-release-vector-metadata-names/verification.md)。

`search_replace` 在明确 NotFound 后通过本地文件系统的独占创建提交完整新文件；既有文件的普通替换和空文件更新使用读取字节条件提交，并以目标文件 advisory lock 串行化 Grow 写者。ACP 文件系统缺少条件写入能力时明确失败；不遵守 advisory lock 的外部写者仍可能在比较与写入间竞争。见[编辑创建契约](../openspec/specs/tool-authorization/spec.md#requirement-search-replace-creation-requires-confirmed-absence)和[既有文件提交契约](../openspec/specs/tool-authorization/spec.md#requirement-local-search-replace-commits-compare-the-source-bytes)。

技能 add/remove 在修改配置前验证请求 cwd 可读取；缺失路径保留锚定后的 `..`，不能据已删除符号链接反推旧目标。remove 无匹配项会明确报告未匹配，可用添加时返回的规范路径移除已保存项。路径身份契约见[配置规则](../openspec/specs/configuration-rules/spec.md#requirement-skill-management-compares-resolved-path-aliases)。

技能发现的 scope 由入口根和规范文件目标共同决定：根内文件保留入口来源，越出根的递归链接按目标位置降级且不能提升优先级。细节见[技能来源别名契约](../openspec/specs/configuration-rules/spec.md#requirement-skill-source-aliases-cannot-increase-precedence)。

`grow/skills/config` 的来源摘要含 Git 与目录探测，和技能重载共用单许可阻塞 worker；两个阶段分别有 5 秒等待期限。超时返回错误，已卡住的文件系统调用仍持有许可直到退出，不能把请求结束当作 worker 已停止。契约见[来源摘要执行边界](../openspec/specs/configuration-rules/spec.md#requirement-skill-configuration-source-summary-uses-the-bounded-discovery-worker)。


## 会话恢复与异步 Context 回归

light load 在 pinned storage 边界完成 Timeline、prompt blob 与 sideband 校验后，将同一份 Timeline 转移给 actor bootstrap；提交输入、控制回执、稳定 System 和用量校验仍在 actor 发布前完成。Workflow 恢复只保留所需 run 的生命周期快照，其修复持久化仍晚于 ChatState 验证。Grow-only UI 事实扫描跳过 ACP payload 的 typed decode，回放顺序、cursor 和最终 load 完成屏障保持。

Hook 的事实来自 Timeline，`HookExecution` 只是 transient 展示投影。恢复补发显式标记 `is_snapshot`，经 passive 路径发送，不执行 handler、不再写持久事件、不关闭 rewind 窗口。Pager 保留 ACP tool id 到展示行的归属，完成工具和合并 Edit 都可接收后到的 Hook；相同 occurrence 只接纳一次。没有可靠工具行的历史生命周期、隐藏工具和说明集中在可展开的 `Restored hooks` 中，历史 stop 不进入当前回合的 stash。该记录没有声称恢复两个日志间的原始全序，见 [Hook 归属契约](../openspec/specs/client-surfaces/spec.md#requirement-hook-projections-preserve-tool-ownership-across-resume) 与 [快照只读契约](../openspec/specs/extension-runtime/spec.md#requirement-hook-history-publication-is-observational)。

已知用量 Observation 在恢复时必须能够解码；同一 attempt/child 结算只接受完全相同的重复，冲突或损坏在 actor 发布前拒绝。未知诊断 Observation 保留扩展性。行为见 [用量恢复契约](../openspec/specs/session-timeline/spec.md#requirement-session-usage-is-a-durable-lifetime-projection)。

异步 Context 请求随结果携带 session id、session binding epoch 与 modal nonce。任何状态更新前同时核对归属；同会话弹窗关闭重开也会使旧结果失效。nonce 为 0 的显式命令仍写入 scrollback，见 [Context 结果归属](../openspec/specs/client-surfaces/spec.md#requirement-context-info-results-are-bound-to-their-requesting-session-view)。

恢复性能入口使用隔离 loopback 配置与合法 Timeline，合成 fixture 在生成前检查 256 MiB 预算。先串行编译，再在没有并行构建的情况下运行；同名 instrumentation 阶段汇总，`session.replay.read_snapshot` 单独记录文件读取。示例：

```sh
CARGO_BUILD_JOBS=2 cargo test --locked -p shell --features test-support --test session_load_perf --no-run
GROW_PERF_TURNS=128 GROW_PERF_AGENT_CHUNKS_PER_TURN=8 GROW_PERF_AGENT_CHUNK_LEN=4096 cargo test --locked -p shell --features test-support --test session_load_perf full_session_load_e2e -- --ignored --nocapture
GROW_PERF_ASSERT_HISTORY=1 cargo test --locked -p shell --features test-support --test session_load_perf full_session_load_e2e -- --ignored --nocapture
```

`GROW_PERF_ASSERT_HISTORY` 单独验证 load response 前历史文本的完整顺序，不用于性能对照。shell 的第一条通知可能是控制通知，不能当作第一条可见历史；完整 ACP load 时间也不能代替真实终端输入回显。Pager 加载期由 `loading_replay_ignores_input_and_preserves_existing_draft_until_session_loaded` 检查普通键/粘贴被忽略、旧草稿保留和完成后恢复输入；`loading_replay_keeps_confirmed_ctrl_d_and_resize_available` 检查退出确认与 resize。历史按实际速度推进，沿用自动重绘节流，不加入播放延迟。行为见 [replay 加载契约](../openspec/specs/client-surfaces/spec.md#requirement-cold-replay-paint-work-is-bounded-by-a-visible-progress-cadence)。终端恢复与后续提交由 ignored PTY `continue_resumes_session_with_history` 检查。

冷 writer load 在发布 actor 前按统一 Timeline journal 中全部展示事实的最大 `eventId` 后缀恢复进程计数器。该边界独立于 `noReplay`、游标和 rewind 展示；被 rewind 隐藏的物理行仍占用身份，exact append 对不同 payload 的重复身份继续拒绝。契约与当前变更见 [冷加载事件身份](../openspec/changes/archive/2026-10-19-reseed-cold-session-event-identities/specs/session-timeline/spec.md#requirement-cold-writer-identity-recovery-is-independent-of-client-replay)。

长会话的真实终端回显可用 ignored PTY 基准取样：先构建当前 `grow`，再运行 `PAGER_BINARY="$PWD/target/debug/grow" cargo test --locked -p pager --test pty_e2e_persistence resume_input_latency_128_512_turns -- --ignored --nocapture --test-threads=1`。它在隔离的 Grow home 和本地 mock provider 中准备 128、512 turns，报告 `--continue` 到末尾历史可见的时间及连续 20 次按键回显 p95。该合成样本不代表密集工具、多子会话、resident/cursor 或真实慢终端；这些场景需要分别取样，不能用单次结果宣称已满足全部长会话验收条件。

可见会话处于历史 replay 时，Pager 对 ACP、动画和周期性 UI maintenance 的自动绘制采用至少 100 ms 间隔；退出确认与 resize 维持即时处理，普通加载输入按契约忽略，`SessionLoaded` 后恢复正常间隔与输入。状态维护和通知处理不受绘制节流影响；契约见 [冷恢复绘制节奏](../openspec/specs/client-surfaces/spec.md#requirement-cold-replay-paint-work-is-bounded-by-a-visible-progress-cadence)。

Pager 在恢复 batch 中保留已有布局，新增条目和 dirty 文本先使用估计高度，再由可见区域完成精确测量；隐藏 thinking 的跨条目间距回退到完整重建。turn 索引在最外层 batch 结束时重建，完整加载后的布局仍预热上方页面。加载中的导出与 transcript 保护继续以 [客户端契约](../openspec/specs/client-surfaces/spec.md) 为准。

Pager 的选中框和悬停框按相邻条目、面板的实际间距放置角符号。无空行时角符号留在当前条目或面板内；有空行时可使用空行，视口裁剪时保留延续边线。粘性标题位于滚动区首行时也收进该区域。契约见 [TUI 框角边界](../openspec/specs/client-surfaces/spec.md#requirement-tui-frame-corners-remain-within-their-owned-rows)。

JSONL full/light observation 仅从已验证 Timeline 派生内存 title/model；显式 writer restore 获得 lease 后才修复持久投影。观察不会因 Summary 滞后而争抢 writer lease，冲突和损坏仍拒绝，见 [只读观察契约](../openspec/specs/session-timeline/spec.md#requirement-session-observation-does-not-repair-durable-projections)。Workflow 从最新候选向前验证，最多保留 128 个有效 run，返回时保持 Timeline 顺序；无效候选不占名额。单文件读取预算保持，但最坏情况下会检查所有 Timeline 候选，见 [Workflow 恢复契约](../openspec/specs/workflow-execution/spec.md#requirement-workflow-restore-retains-the-latest-valid-runs)。

`grow/workflows/list` 将项目和用户目录扫描交给有界 blocking worker，五秒期限包含等待扫描资格；请求超时后无法中断的文件系统调用仍由 worker 持有许可直到退出，worker 错误与超时都返回 RPC 错误。见 [Workflow 列表执行边界](../openspec/specs/workflow-execution/spec.md#requirement-workflow-listing-has-a-bounded-execution-boundary)。

无 session 的非 chat `grow/commands/list` 将信任解析、插件配置与发现、技能及 Workflow 目录扫描放入同一共享单许可 worker，五秒期限覆盖排队和扫描；超时不代表后台扫描已停止，worker 退出前仍持有许可。Chat catalog 和 session 自有命令快照沿原路径返回。见 [pre-session 命令目录执行边界](../openspec/specs/client-surfaces/spec.md#requirement-pre-session-command-discovery-has-a-bounded-execution-boundary)。

Pager 的 `/config-agents` 先打开可关闭的 Loading 弹窗，再以单许可后台 worker 扫描 Agent 定义。五秒期限覆盖等待和扫描；超时只结束 UI 等待，阻塞线程退出前仍占用许可。扫描结果以弹窗加载 token 对账，关闭重开或外部编辑器刷新后，旧结果不能覆盖当前目录；成功切换启用状态只更新已写入的行。见 [Agents 弹窗发现契约](../openspec/specs/client-surfaces/spec.md#requirement-agent-configuration-modal-discovery-does-not-block-the-event-path)。

PromptWidget 创建和 `/agent` picker 打开时只装载静态内建 Agent，普通会话的项目、用户与插件 Agent 由同一 Pager 发现许可和五秒期限的后台任务补齐。结果核对可见 Agent、session binding 与 picker 请求；Workflow child 始终使用 Run 冻结快照，直接输入 `/agent <name>` 仍由 Shell 最终判定。见 [Agent 切换发现契约](../openspec/specs/client-surfaces/spec.md#requirement-switch-agent-discovery-does-not-block-pager-input)。

重复选择当前 Agent 或相同 provider/model/effort 仍完成正常控制回执，但不显示 Switching 或成功切换提示；相同 Agent 名称对应的有效配置改变、effort 改变以及失败仍按真实结果显示。见 [未改变选择的展示契约](../openspec/specs/client-surfaces/spec.md)。

忙碌会话的 `/effort` 可先收到 `ModelChanged`，再收到对应控制终态。Pager 在本地 Sampling intent 未结算时暂存模型投影；匹配的终态回执解除该 intent 后，通知处理路径立即应用暂存投影，即使 Agent 或 Behavior 控制仍在等待。RPC 成功只确认请求，不能代替这个状态同步。见 [忙碌 Sampling 投影契约](../openspec/specs/client-surfaces/spec.md#requirement-busy-sampling-control-terminals-release-matching-model-projections)。

Messages 在单次请求内为中性工具 ID 建立映射，预留已有合法和 native ID，并同步编码调用与结果。原生 continuation 的 ID/签名保持，Timeline 不改写，见 [工具身份编码契约](../openspec/specs/model-sampling/spec.md#requirement-messages-tool-identity-encoding-preserves-distinct-exchanges)。64 字节是 Grow 的中性 ID 编码预算。

工具定义及模型图片资格 key 由 `tool-types` 持有，客户端身份值由 `config-types` 持有；HTTP 不引用 sampler/workspace 运行时。Pager 负责识别权限特殊行，renderer 只接收对应位置。版本策略由 `config::VersionPolicy` 持有，更新器的配置值和持久化操作由 CLI 组合层传入，仍复用 shell 的完整配置读写规则。Core regression 对这五条直接依赖边设有检查，不因此推断二进制或构建耗时的改善。

`bounded_replay_text_blocks_and_prompt_measurement` 使用 128/256 轮、每轮 8 个 512-byte chunk，经过真实 ACP handler 和 TextBlock，每 32 条通知准备布局并穿插被忽略的 loading 输入。它分别报告通知、布局、输入处理、load 收尾和最终布局耗时，并检查历史完整性、旧草稿保留和没有新增排队输入；这是进程内阶段测量，不包含终端绘制或设备回显。

```sh
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p pager --lib bounded_replay_text_blocks_and_prompt_measurement -- --ignored --nocapture --test-threads=1
```

恢复 PTY 探针已接入 `pty_e2e_persistence`：`resume_history_progress_with_slow_writer` 和 `resume_dense_history_progress_with_slow_writer` 用递增可见标记记录首条历史、推进次数和最大内容间隔，并核对 loading 输入被忽略及 ready 后输入回显；`ctrl_d_exits_during_session_loading` 确认加载中退出后还能恢复原历史。运行时沿用上述 `PAGER_BINARY` 方式、`--ignored --nocapture --test-threads=1`；先用 `--list` 确认精确测试存在，避免过滤到零项。性能最终结论见四项归档：[Timeline admission](../openspec/changes/archive/2026-10-21-optimize-timeline-admission-validation/verification.md)、[projection append plan](../openspec/changes/archive/2026-10-21-reuse-response-projection-append-plan/verification.md)、[布局与加载交互](../openspec/changes/archive/2026-10-21-optimize-replay-layout-maintenance/verification.md)、[exact commit 索引](../openspec/changes/archive/2026-10-21-index-exact-update-commits/verification.md)。历史数据不作为当前构建的速度承诺。

### Prompt cache 离线诊断

完整会话快照可用 `python3 scripts/analyze_prompt_cache.py --session-dir <snapshot> --baseline <request-id:attempt> --request <request-id:attempt> --format text|json` 对比已保存的 provider 请求证据。主请求使用实际 `request_id:attempt_number`；Sideband 请求可用 `sideband_id:sideband_attempt`。工具读取 `timeline.jsonl` 与其引用的 `artifacts/sampling` chunks，只物化所选请求及其可得响应，验证长度和 BLAKE3，并拒绝符号链接及越出快照的证据路径。默认输出大小、digest、结构路径、缓存字段 presence 与原始响应的白名单 usage 数值，不输出请求正文或工具参数。快照只记录去敏 endpoint，不能据此确认租户和部署身份；因此报告分别展示可见 endpoint 是否相同与 provider 级比较不可确认。可见 JSON 前缀只说明本地结构相同，不证明 provider 缓存命中；报告将 Timeline 的已结算 Request usage 与原始响应证据分开显示。该工具仅用于完整、已静止的快照，不读取写入中的一致性快照，也不推断服务端缓存状态。

请求图片预算在 actor 内按 Surface 身份保留已选 placeholder；纯追加先重放原选择再测量，高水位才扩大回收。`source_projection.image_budget.evicted_parts` 是本轮完整生效选择，`newly_evicted_parts` 仅是本轮新增。原始 Timeline 图片不受请求预算修改；native epoch 与 provider cache 命中需分别核对。契约见 [图片预算选择](../openspec/specs/model-sampling/spec.md#requirement-image-budget-choices-remain-stable-between-reclamation-boundaries)。

采样用量的 cache read/write 分别保留「缺失」与「明确为零」；会话总输入包含缓存命中，缓存率只以 read 已知的输入为分母，`/usage` 另显示这部分占全部已记录输入的覆盖率。总输入可信但 cache 分类未知时，账本总量仍精确，Goal 将该输入列为未分类并按全量计入预算。ACP/headless 的结构化用量携带已知桶、覆盖分母与未知调用数；headless 旧的 uncached `input_tokens` 无法准确派生时为 `null`，`full_input_tokens` 仍给出可信总输入。契约见 [缓存用量可用性](../openspec/specs/model-sampling/spec.md#requirement-cache-usage-preserves-field-availability-across-protocols)、[会话用量展示](../openspec/specs/client-surfaces/spec.md#requirement-usage-exposes-provider-model-totals-and-cache-hit-rates) 与 [Goal 分类](../openspec/specs/behavior-goal/spec.md#requirement-goal-usage-exposes-provider-style-components)。

树级挂起在向 coordinator 冻结前确认 root 的持久 operation；初始化超时或 ACK 丢失后按原身份重试。人工等待折叠以 `ExecutionReleased` 隔离恢复周期，覆盖 child 提前冻结、节点意图尚未写入时的 `Suspended` 结算。见 [挂起准入边界](../openspec/specs/session-timeline/spec.md#requirement-tree-admission-freeze-has-a-durable-suspension-identity)。自动 Goal 激活与定义重激活从既有 BehaviorCoordinator 快照派生目标 Control，保留候选、完成引用和提交计数；见 [Plan 身份契约](../openspec/specs/behavior-goal/spec.md#requirement-behavior-selection-preserves-durable-plan-identity)。

恢复后的人工等待以匹配 checkpoint 中的 `AwaitUser` / `AwaitPermission` 延续到新 attempt 建立；`ExecutionReleased` 只放行执行，不代表等待已回答。再次挂起时必须保留这段逻辑等待，真实解决与工具结果仍优先。见 [恢复与再次挂起](../openspec/specs/session-timeline/spec.md#requirement-suspension-wait-evidence-spans-admission-closure)。局部规划切换 scope 复用 `LocalPlan::validate_transition_from`：明确未派发的可选 link 可处置，必需交付仍需 Released review，活动或未知执行不能靠 review 伪装为已完成；见 [scope 处置规则](../openspec/specs/task-planning/spec.md#requirement-scope-switches-accept-explicitly-disposed-dispatch-failures)。

精确测试 runner 的 compile 阶段用 Cargo JSON 确认唯一 libtest 产物，enumerate/execute 直接运行同一产物并保持 package manifest 所在目录；枚举不再通过 Cargo 触发重编译。诊断独立保留，缺失/多个产物或零测试仍失败。契约见 [固定测试产物](../openspec/specs/development-workflow/spec.md#requirement-exact-cargo-selections-stay-pinned-to-one-compiled-test-executable)。

Settings 的 Agent 类别提供独立的 `Subagent permission mode`（Ask / Auto / Always approve，默认 Auto），写入 `[subagents].permission_mode`。该项重启后生效，沿 Settings 顺序保存与失败回滚；它与主 Agent 的默认权限及当前会话权限分开。契约见 [子 Agent 权限设置](../openspec/specs/client-surfaces/spec.md#requirement-settings-exposes-independent-subagent-permission-policy) 和 [定向配置写入](../openspec/specs/configuration-rules/spec.md#requirement-subagent-permission-settings-update-only-the-authored-policy)。

完整 Settings 菜单集成回归使用 `cargo test --locked -p pager --test settings_e2e -- --test-threads=4`，覆盖注册清单、默认值、类型分支及键鼠交互；该套测试与 library 回归共同纳入 Core regression，正式发布时在同一已验证 SHA 上执行。

Settings 的完整键鼠回归由 `cargo test --locked -p pager --test settings_e2e -- --test-threads=4` 执行；保存调度与重置失败回归由 `cargo test --locked -p pager --lib settings -- --test-threads=4` 执行（沿现有测试栈配置）。有效默认与失败后重试行为见 [偏好回滚契约](../openspec/specs/client-surfaces/spec.md#requirement-preference-rollback-uses-the-displayed-effective-default)。


现代恢复基准在同一入口设置 `GROW_PERF_SESSION_FORMAT=modern`，构造合法 admission、payload-free anchor、canonical projection 和匹配当前 mock endpoint 的模型路由基线；每轮 canonical 正文为 `AGENT_CHUNKS_PER_TURN × AGENT_CHUNK_LEN`。用 `GROW_PERF_MISSING_PROJECTION_EVERY=16` 单独测 writer repair，用 `GROW_PERF_OMIT_ROUTE_BASELINE=1` 显式测缺基线的恢复写入，避免把该成本混入正常样本。测量窗口只包含 load RPC，`PERF_RESUME` 输出历史字节、首条历史、结束时间及 count/sum；父子 inclusive timers 不直接相加。各规模三次独立进程，完整历史 oracle 单独运行。

PTY 对应 `resume_modern_admission_projection_128_512_turns` 和 `resume_modern_history_progress_with_slow_writer`，沿用 `PAGER_BINARY` 和精确枚举/执行方式。探针分别测末尾历史首次可见与实际 SessionLoaded 边界；模型标签或稳定画面不足以判断输入已经启用。热点定位与CPU采样见 [现代恢复分析](../openspec/changes/archive/2026-10-21-profile-modern-session-resume/analysis.md)。R5 writer前缀恢复复用私有bulk fold，仍对完整原始字节验证hash、seq及lifecycle；R6将同次writer规划的canonical集合移交初次回放，模型路由补写后弃用，resident/delta不跨快照复用。raw仍逐条核对exact payload，借用的重排行保留合成来源和cursor回退语义。基准与验证分别见 [R5](../openspec/changes/archive/2026-10-21-optimize-timeline-writer-prefix/verification.md)、[R6](../openspec/changes/archive/2026-10-21-reuse-resume-canonical-projections/verification.md)。现代中途退出再恢复用 `ctrl_d_exits_during_modern_session_loading`。这些是合成 dev/test 数据，不是 release 延迟承诺。

Responses 下用户主动加载的技能以独立 user 上下文传入，保留结构化来源、加载身份及完整正文；不再生成需要模型 reasoning 的模拟 `load_skill` 调用。真实模型技能调用仍使用原工具往返。排查 thinking `reasoning_text` 400 时，应区分历史 reasoning 缺失与 Runtime 事实被错误编码成模型调用。见 [技能请求投影](../openspec/specs/model-sampling/spec.md#requirement-runtime-skill-results-have-legal-provider-projections)。

技能发布的 headroom 检查沿用 ChatState 的 `projected_tokens`，与状态栏、请求投影和普通上下文压力检查保持同一口径，再计入完整新增内容。不要用原始 Surface 的 bytes/4 总量替换现有计数：原始历史含可能不随请求发送的 reasoning，也不包含完整 provider envelope 开销。见 [技能发布预算计数](../openspec/specs/model-sampling/spec.md#requirement-skill-publication-uses-projected-request-context-pressure)。

冷恢复会重建 native continuation lane，初始上下文压力按现有 portable replay 的消息贡献估算，计数始终使用有效请求估值，不减去原始 Surface 的差分。后续请求计入当前工具和 schema 等 envelope，并保留同一路由的 provider 校准状态；恢复估值不复用旧 live anchor，也不截到窗口上限。见 [冷恢复上下文计数](../openspec/specs/model-sampling/spec.md#requirement-cold-context-pressure-follows-portable-replay)。

resident 会话的 rewind 在 Timeline durable ACK 后、真实采样路由替换在切换后，同样按新 lane 的 portable replay 重建压力和请求估值基线，并立即通知状态栏。跨压缩 rewind 恢复的原始 reasoning 继续保存在历史中，只有当前 replay 规则实际发送的部分计入压力；普通参数更新和 compaction 保留同一路由的校准状态；replacement、prune、图片投影与 native reset 后重新估算实际回放内容，重复 request build 不重复调整。见 [rewind 与路由切换计数](../openspec/specs/model-sampling/spec.md#requirement-rewind-and-route-replacement-rebase-context-pressure-to-portable-replay)。

压缩与回合分别结算：replacement durable ACK 后，即使剩余输入仍超窗而终止回合，也须发送 Completed 并立即作废旧的异步压缩通知。提交前失败或取消发送对应终态；Pager 回合结束只清理 compaction 域反馈。普通请求在完整投影后独立检查当前模型窗口，重试抑制和子任务输出预算不能放行明确超窗输入；模型切换压缩失败向上返回。一次有界摘要后仍超限会保留历史并结束本轮，不靠无限重试或丢弃未摘要内容收敛。见 [上下文有效计数](../openspec/specs/model-sampling/spec.md#requirement-context-mutations-use-effective-request-estimates)、[硬窗口准入](../openspec/specs/model-sampling/spec.md#requirement-known-input-overflow-cannot-bypass-provider-admission) 与 [压缩契约](../openspec/specs/context-compaction/spec.md)。

Runtime 收件消息在 Responses 中同样使用带来源的独立 user 上下文，明确不是直接用户指令或人工授权；Chat/Messages 保留完整收件工具对。Chat 路由确认 reasoning_content 拒绝后，统一为所有缺字段的 assistant 编码空字段，包括 portable 与 live Runtime 投影，不生成思考正文。模型技能结果间的匹配 SkillLoad 元数据在请求前稳定归整至结果批次之后，原始 Timeline 事实不变，真实 sibling 结果不得被取消说明覆盖；旧 IntegrityRepair 保留原精确重放证明。契约见 [Runtime 收件投影](../openspec/specs/model-sampling/spec.md#requirement-runtime-agent-messages-project-to-complete-tool-exchanges) 与 [结果证据保全](../openspec/specs/session-timeline/spec.md#requirement-integrity-repair-preserves-evidence)。回归入口为 sampling-types 的 `runtime_projection`、chat-state 的 `model_skill_integrity` 及 actor 的 `model_skill_batch_preserves_success_through_request_and_timeline_replay`。

`/btw` 只读复用主会话的图片文字分支：等待已启动的主转换及其 ImageProjection durable ACK，再冻结 Sideband 上下文并优先选择已有描述（不依赖负能力缓存是否命中）。它不启动视觉辅助/OCR、不写图片能力、不因图片拒绝重建主会话；读锁在 HTTP 请求前释放。已知纯文本路由的未转换图片以未就绪失败返回。问题及回答仅归属独立 Sideband。见 [Side question 图片分支复用](../openspec/specs/model-sampling/spec.md#requirement-side-questions-reuse-durable-image-fallback)。

全屏 TUI 的 `/btw` 回答使用覆盖式大弹窗；Esc 收起后双击历史行可在同尺寸查看。Minimal 模式继续使用输入框上方的原生终端面板。界面契约见 [侧问结果弹窗](../openspec/specs/client-surfaces/spec.md#requirement-fullscreen-side-question-answers-use-a-large-popup)。

Resume 的首历史归因沿用 opt-in instrumentation：`session.replay.first_send` 标记工具消息合并后的首次 gateway 转发发起，须与相同 session/cursor 的 `pager.replay.first_applied` 配对确认交付。`pager.replay.first_frame` 是应用历史后的首个普通根会话帧，其 `writer_sequence` 与成功 `pager.frame.delivery`、`frame_ack.sequence` 关联；不是该 source row 首次出现在物理屏幕的证明。子页面、Dashboard/覆盖层不按普通根帧归因，失败/空历史不伪造完整链；终端可见时间继续由 PTY 独立观察。标记不改变 100ms loading 绘制节奏、顺序、Ctrl+D 或完整历史就绪屏障。

摘要来源预算由实际发送的历史投影计算；完整边界和 replacement target 仍取原 Surface IDs。未发送的 reasoning 不占来源预算，输入超长后的缩小重试也使用同一投影。摘要 adapter 必须看到明确成功终态，截断、拒绝、暂停、未知终态或 EOF 缺少完成证据均不能替换历史；Responses 只在真实 `response.completed` 时记录成功。手动压缩排队后，普通 turn 在闭合边界交出前台，由现有 idle 仲裁启动；后台压缩普通取消直接信号化原 generation token，显式 drain 才等待其结算。参见 [摘要投影预算](../openspec/specs/context-compaction/spec.md#requirement-summary-source-fitting-uses-the-transmitted-history-projection)、[摘要终态验收](../openspec/specs/context-compaction/spec.md#requirement-only-completed-summary-responses-may-replace-history) 和 [排队手动压缩](../openspec/specs/context-compaction/spec.md#requirement-queued-manual-compaction-yields-at-a-closed-execution-boundary)。

压缩等待方退出不会创建新的 generation。再次等待同一已提升且尚未提交摘要的 worker 时，ChatState 的 durable 入口返回原已确认 `Promoted`；未确认 ACK 仍由原写入流程完成，失败则关闭 writer，不能按日志可读性提前承认成功。故障注入将 replacement、Completed 和 Control 重投影分别核对，冷恢复按保留事实闭合事务。路由切换后的异步完成通知等下一请求投影；新的 replacement 作废旧通知。见 [取消与 owner](../openspec/specs/context-compaction/spec.md#requirement-cancellation-preserves-and-drains-the-compaction-generation-owner)、[持久确认边界](../openspec/specs/context-compaction/spec.md#requirement-compaction-persistence-uncertainty-preserves-committed-boundaries) 和 [异步展示](../openspec/specs/context-compaction/spec.md#requirement-async-compaction-completion-reports-a-materialized-request-projection)。

压缩的跨 ledger 校验按 Summary 所属事务查找 replacement：合法 Failed 可以留下未发布摘要，后续对相同 SurfaceRange 的重试属于新事务。真实文件/进程回归为 `cargo test --locked -p shell --lib compaction_real_files_process_recovery -- --test-threads=1 --nocapture`；测试在隔离 Grow home 中截住真实 durable writer 的返回 ACK，由父进程终止子进程，再用新进程运行 writer load、Timeline 恢复和 Control 修复，并核对重复恢复的文件字节不变。它不依赖线上模型，也不替代断电测试。契约见 [压缩持久化边界](../openspec/specs/context-compaction/spec.md#requirement-compaction-persistence-uncertainty-preserves-committed-boundaries)。

请求的活动 Goal 投影作用域由 ChatState 请求状态持有，独立于图片选择缓存；冷恢复从持久化授权初始化，运行期随同一 Control durable ACK 切换。pause、完成、budget stop、clear 和 Behavior 切换移除授权，restart 与定义 revision 使用新的 tag；失败写入不发布新 scope。主请求沿既有 Step control gate 串行化 scope 读取、图片资格查询和请求投影，避免旧 tag 越过较新的 Control。换路由仍清 provider 校准，图片缓存失效保留现有校准语义。见 [有效请求计数契约](../openspec/specs/model-sampling/spec.md#requirement-context-mutations-use-effective-request-estimates)。

完整可信的 provider context 回执即使低于本地 heuristic 也成为计数锚点。校准只保留一份随当前投影推进的参考：追加按本地增量计入；低值 footprint 缩小时按当前比例向上取整；再次增长不能继续套用压缩前比例。route/rewind 清校准，累计账本不变。冷恢复从持久化 Behavior/Goal 授权初始化请求作用域，并在运行时协调后同步最终 scope，随后才公布会话。普通和技能请求统一以已校准输入加 task clamp 后的显式输出上限检查总窗口；None 不虚构后端默认预留。见 [有效计数](../openspec/specs/model-sampling/spec.md#requirement-context-mutations-use-effective-request-estimates)、[冷恢复](../openspec/specs/model-sampling/spec.md#requirement-cold-context-pressure-follows-portable-replay) 和 [窗口准入](../openspec/specs/model-sampling/spec.md#requirement-known-input-overflow-cannot-bypass-provider-admission)。

同一 turn 在 Step 边界实际换路由后，已获准的下一 Step 通过持久化 AutoContinue 接手保留历史。该内部 user 边界不增加用户授权或重新执行工具；同路由 effort 变化不追加，已有异步压缩续接则复用，写入失败不派发下一请求。跨 provider 的“缺少 reasoning”拒绝可能来自不兼容的旧工具调用 ID，不能据此认定全部历史 reasoning 都会占用新模型窗口。仍保留独立的显式输出预留检查。见 [跨路由续接](../openspec/specs/model-sampling/spec.md#requirement-cross-route-continuation-hands-off-completed-history-at-an-admitted-step)。

显式输出预留造成总窗口超限时，普通前台请求沿同一自动压缩入口先跨 Step 结算边界恢复；trigger 保留实际输出上限，图片重投影和 prune 复核也按总需求判断。prune 目标同时受输入阈值和 `context_window - max_output_tokens` 约束；一次摘要或后台提升提交后仍放不下时，保留 Completed 并终止本轮，不重复发摘要。未设置输出上限时不猜默认值，输出预留自身已占满窗口时不为该预留启动无效恢复。抑制与子任务准入规则不变。见 [总窗口准入契约](../openspec/specs/model-sampling/spec.md#requirement-known-input-overflow-cannot-bypass-provider-admission)。

恢复后的 `/goal clear` 与其他 Goal 控制共用同一 Control 提交入口；事实和请求 scope 在一个 durable ACK 边界发布，不再用后续独立 setter 清除。持久化失败保留原 scope。用户 rewind 在 Goal 状态存在时仍要求先 clear，内部 ChatState 历史重放不替代这一入口约束。见 [Goal 与上下文组合契约](../openspec/specs/model-sampling/spec.md#requirement-context-mutations-use-effective-request-estimates)。


ACP 的配置和技能 watcher 由 blocking runtime 注册，gateway 先开始处理协议。配置注册就绪即处理排队的路径订阅、启动 reload consumer，并协调启动间隙已建立 session 的当前 catalog/policy，不等待技能注册。取消或 gateway 结束可停止 LocalSet 等待，尚未完成的系统注册由 runtime 保有最终结果并释放；操作系统调用本身不可取消，runtime shutdown 仍可能等待它返回。见 [注册边界](../openspec/specs/client-surfaces/spec.md#requirement-ready-acp-service-is-not-blocked-by-watcher-registration)。

SideQuestion 的任务同时监听响应接收端关闭；关闭后释放原请求 future，由原 Sideband guard 结算用量和终态，不另开取消账本。Plan live handoff 在 pending 已空时核对精确 source/version 的持久消费 Turn；仅当前 Turn 的重复核对幂等成功，其他 Turn、Dismissed 或不可用 owner 不得放行。见 [SideQuestion 生命周期](../openspec/specs/model-sampling/spec.md#requirement-side-question-work-follows-its-response-receiver) 与 [Plan 交接](../openspec/specs/behavior-goal/spec.md)。

子会话恢复中的 `CompletedOutputUnavailable` 只是一项观察。已有合法 canonical Result 的 outcome 仍保留；没有合法 Result 时不合成成功，也不闭合 parent spawn。后代展示的 metadata v2 携带真实 typed parent Spawn 及匹配 terminal，复用标准 Seed/Result 交叉校验；展示身份不授予执行权。无 eventId 的 Started 历史复用相同 child 的任务行。见 [子会话恢复](../openspec/specs/session-timeline/spec.md) 与 [展示恢复](../openspec/specs/client-surfaces/spec.md)。

Task 的拒绝区分未知完整 ID、全局 toggle 禁用和父 Agent 委派限制；嵌套定义以目录相对路径（不含扩展名）为 ID，frontmatter 的短名不会自动成为别名。全局 `[subagents.toggle]` 由 daemon 与 in-process Pager 共用的 reload 入口更新：新派发读取当前快照，已有普通主/子会话在下一 Step 或空闲控制边界重建目录，全部关闭后重新开启也会恢复 Task。删除条目恢复默认开启，非法值保留上次有效策略；父名单、深度上限、已运行子任务和 Workflow 冻结路由保持各自约束。`enabled`、`max_depth`、`permission_mode` 不属于此次热更新范围。见 [拒绝归因](../openspec/specs/tool-authorization/spec.md#requirement-subagent-rejection-preserves-its-actual-policy-cause) 与 [toggle 热更新](../openspec/specs/configuration-rules/spec.md#requirement-subagent-toggles-reload-into-live-admission-and-catalogs)。

委派授权使用解析后的正式 selector：插件短名按限定 ID 检查 toggle 和父 allow/deny，含空段、`.`、`..` 的路径不作为 ID 接受。普通 Task 目录合并 native/plugin/CLI fallback，再应用相同开关；同名高优先级定义被关闭不会暴露 CLI 替代项，CLI 定义也随普通会话重建保留。原挂起 child 从 Spawn 的有效 capability_mode 恢复出生 RWX，再受当前父 ceiling 限制；当前选中 profile 的默认权限不重新授予，缺失或无效出生事实阻止恢复。恢复使用新运行时的逐次授权，不复用 permit。见 [正式身份授权](../openspec/specs/tool-authorization/spec.md#requirement-delegation-authorizes-canonical-definition-identities) 与 [出生权限恢复](../openspec/specs/session-timeline/spec.md#requirement-restored-children-retain-their-original-birth-authority)。

关闭 sticky prompt 或选择 compact prompt 后，精确 top/center 导航使用与绘制相同的 header 几何。直接 `diagnostics::instrumentation::timer` 在 Chrome 模式产生 `instrumentation.timer` span，`name` 字段保留原指标名；Shell 既有字面量 span 路径不重复创建。Pager/CLI 的 Git 构建身份共用元数据路径发现，跟踪真实 worktree HEAD、活动 loose ref 或 packed refs；不存在的嵌套 ref 监听最近已存在目录，`RELEASE_COMMIT` 仍优先。

本地 OCR 复用已有 ProcessGroup/Windows job 所有权，取消、超时、错误和成功返回均终止残留后代；direct child 仍由 Tokio kill-on-drop/wait 负责。PTY Leader fixture 持有专用 `TestProcess`，在 sandbox 之前析构并 reap，模拟断言 unwind 走相同路径。Core 门禁增加 diagnostics、pager-render、tty-utils 与 test-support，并在 CLI 构建后精确运行真实 daemon 普通退出/断言失败清理回归；tmux 的 post-exit 清理预算测试以握手和已过期主期限区分独立宽限期。构建/运行方法沿用上述测试入口与 `PAGER_BINARY`。
