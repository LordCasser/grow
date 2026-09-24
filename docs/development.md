# 开发流程

项目使用 OpenSpec SDD。先把本次要改变的行为写清楚，再实现和验证，最后归档成为当前规范。纯文档修正也保留最小变更记录，但不增加无意义需求。

采样断流排查从 attempt evidence 的 `attempt_number`、`stream_end`、
`output_delivery`、`output_observed` 和 `recovery_stop` 决定开始，再核对
Timeline 的 `sampling_usage/attempt_settled`。`[DONE]` 或 body EOF 不能代替
协议完成证据；没有对应会话的原始尾帧时，不能仅凭错误文字归因于代理。
HTTP 200 的 SSE `event:error` 要连同事件名核对；平铺的 `code/message/request_id`
是 provider 错误事实，普通事件缺少 `type` 则仍是协议错误。内容检查拒绝不自动重试，
限流和过载仍须通过统一 attempt 安全与预算门槛。契约见
[named stream error](../openspec/specs/model-sampling/spec.md#requirement-named-provider-stream-errors-retain-their-error-facts)。
当前 `GROW_MAX_RETRIES` / `max_retries` 沿用字段名，表示同一未接纳模型步骤的
总 attempt 上限，包含初次调用；0 仅允许初次调用。session 默认上限为 5，
独立 sampler 默认值为 15。共享期限取首次 idle timeout × 总上限，后续修复
不会延长；精确预算或不可撤销输出可能更早关闭恢复。
模型切换后的 thinking 400 需核对错误要求的是 Chat Completions `reasoning_content` 还是 Responses `reasoning_text`。明确回传拒绝通过 typed backend 事实交给 ChatState，只有匹配的当前路由可确认启用一次；请求证据的 `portable_reasoning_backend` 记录这一选择。Chat 将紧邻 assistant 的可见 reasoning 编码回传，无文本时传空字符串；Responses 仍仅回放完整工具往返前的 reasoning。原生签名、加密内容不从历史重建，同路由 reset 保留已学习规则，真实路由替换清除规则。契约见 [portable reasoning 恢复](../openspec/specs/model-sampling/spec.md#requirement-provider-required-portable-reasoning-recovery-is-bounded)。
实现导航见 [采样恢复边界](architecture/session-robustness-repair.md)，
行为以 [model-sampling](../openspec/specs/model-sampling/spec.md) 为准。
`sampling_evidence/recovery_stop` 是停止自动恢复的合法历史证据，与 request、response、retry 一起经过加载及导入导出的完整性校验；不能删除该记录来绕过加载失败。契约见 [停止恢复证据](../openspec/specs/session-timeline/spec.md#requirement-sampling-recovery-stop-evidence-survives-session-storage)。
Sampler graceful shutdown 会先停止准入并取消 provider 工作，再等待已准入 request task 完成 attempt evidence 与所有适用用量账本的确认结算；不能用 abort 或缺失 ACK 跨过该边界。若现有 `SamplerOwner::shutdown_bounded` 的明确 deadline 到期，owner 才会强制终止剩余任务并返回关闭失败，调用方不得把它当作成功的最终持久化 frontier。

子 Agent cancellation token 只发起取消，不能直接授权写入关闭 child Timeline 的 `SubagentResult`。runner 必须等待 child prompt terminal，使已准入 attempt 的 evidence 与 usage settlement 先越过上述 frontier，再提交 child result 和 parent `Ended.result_ref`。完整且严格链接的 `cancelled` lifecycle 可以显式 resume；resume 先拒绝仍由 coordinator 持有的 source，再校验 parent/child storage、seed、identity 和 exact result link。恢复沿用 source 自己的稳定 System head，不依赖当前 head renderer 或历史 completion output artifact；真正被 Surface 引用的 prompt blob 仍须存在。派生 child 只有在 control snapshot、Goal snapshot mailbox（适用时）和首轮 prompt durable ACK 依次通过后才完成启动准入。契约见 [cancelled lifecycle 恢复](../openspec/specs/session-timeline/spec.md#requirement-canonical-cancelled-subagent-lifecycles-are-resumable) 与 [取消结算屏障](../openspec/specs/model-sampling/spec.md#requirement-subagent-cancellation-settles-admitted-attempts-before-closing-the-child-timeline)。

Assistant response admission 由 ChatState Timeline 持有 `request_id + final attempt` identity 和确定性的 quarantine 结果。Shell 将 identity/payload 交给当前 ChatState owner；owner acknowledgement/persistence/causal 错误直接以 `response-admission` turn-boundary fatal error 停止 completion recovery、Accepted 发布和工具执行，不通过失效 owner 重试。cold/replacement owner 可用后，exact identity/payload reissue 幂等返回原结果，不重启 sampler/provider、不重复安装 native continuation。历史 response 的可选 metadata 缺失时仍可读取，但不能用启发式确认新的不明 admission；native continuation 仍为瞬态状态。

Identity-bearing response 在 Timeline admission 后还要经过 Shell 的 `response-replay-projection` durable gate。`updates.jsonl` 中的 versioned projection record 只是可重建 UI cache；投影准备时的 assistant/reasoning 正文与 Timeline 共享 `Arc<str>`，在 replay 或 fork 生成 ACP 通知时才复制为展示文本。流式 candidate 只用于可撤回 live preview，不能成为 accepted history authority。Actor 向持久化队列只投递首个 candidate 的空正文顺序标记，不复制正文；实时网关仍独立接收所有候选通知。每会话的实时预览网关持有最多 4096 个 16 KiB credit，直到网关处理完成才归还；单条通知超限或预算耗尽会使本次 attempt 的 admission 失败。Shell 在 Timeline admission 前刷新 event FIFO 并等待持久化屏障，确认该标记及此前独立事件已写入；attempt 中由会话 actor 发出的独立 ACP/Grow 更新先冲刷待写记录并逐条等待持久化 ACK，不能在 sender 中持续堆积。Projection commit 写在物理末尾，replay 依标记把 canonical 正文放回首个候选位置；标记、独立事件写入或预览预算失败均阻止 admission。commit 经过 session event FIFO，且在 ACK 前禁止发布 Accepted、继续 provider/continuation 或执行工具；quarantine 记录 Discarded disposition 且不保存 raw malformed preview。Replay reader 展开 storage-only record，不将其作为公开 SamplingAttempt 通知。契约见 [session-timeline](../openspec/specs/session-timeline/spec.md#requirement-candidate-preview-uses-a-durable-payload-free-anchor)。

普通会话的 Sampler→Shell 事件交接对 candidate 文本、reasoning、工具参数和响应元数据片段使用每会话 4096 个 16 KiB credit；单个片段超过 64 MiB 会在入队前使 attempt 失败。会话 actor 消费该片段翻译出的事件并确认 FIFO fence 后才归还 credit，因此忙碌的会话事件循环会反压 provider 读取，取消仍可打断等待。ReplayBuffer 的可配置合并阈值被限制在片段预算以内。attempt 边界和终态保留原有 FIFO 顺序；Sampler→Shell 与 Shell→gateway 是独立预算，不等同于进程总内存上限。leader 对不支持撤回的观察者只暂存 candidate 通知；独立 ACP/Grow 通知照常实时投递，因此 live 展示可早于随后接纳的候选，而 durable replay 保留原顺序。该候选按每会话单个临时 spool 保存：8 MiB 内存阈值后落到 socket 同目录的匿名文件，每个候选最多 512 MiB 序列化记录或 100 万条，Accepted 逐条回放，Discarded 直接释放；超限或 I/O 失败会丢弃该 attempt 的临时候选，保留订阅和独立通知；若随后 Accepted，leader 在在途 load 完成后通知 Pager 原地全量回放，Discarded 则无需重载。见 [采样片段交接契约](../openspec/specs/model-sampling/spec.md#requirement-session-sampler-preview-handoff-bounds-fragment-backlog)、[单片段超限契约](../openspec/specs/model-sampling/spec.md#requirement-oversized-sampler-preview-events-fail-before-enqueue)、[独立通知契约](../openspec/specs/client-surfaces/spec.md#requirement-independent-notifications-bypass-retractable-candidate-buffering)和[候选暂存契约](../openspec/specs/client-surfaces/spec.md#requirement-leader-candidate-retention-has-a-finite-spool-budget)。

Goal 与 Workflow 的 Grow 展示快照同样使用该会话网关预览 credit，并在入持久化队列前为其独立副本占用 credit，分别在网关完成和写入完成后释放；慢消费者导致的预算耗尽会跳过展示快照，若发生在活跃 attempt 内则阻止该次 response admission。Goal 控制快照与 Workflow manifest 的权威持久化不依赖这些展示快照。子 agent 的瞬态进度发布者每次只等待一条网关投递，取消可打断等待。契约见 [辅助 Grow 投递](../openspec/specs/session-timeline/spec.md#requirement-auxiliary-grow-delivery-shares-the-session-preview-budget)。

子 agent 的 Spawned/Finished Grow 生命周期通知由父会话 actor 写入后再转发，写入与实时通知复用一个 `eventId`；Finish 展示只携带状态与计数，不复制完整答案。答案保存在经过验证的子会话结果和父会话完成回执中。父 actor 在活跃 attempt 内转发时沿用独立事件 ACK 与预览网关预算；外部客户端送入的 Grow 通知仍只持久化，不回显。契约见 [子 agent 生命周期投影](../openspec/specs/session-timeline/spec.md#requirement-subagent-lifecycle-projections-are-bounded-metadata)。

工具通知桥的 TaskCompleted Grow 投影清空 `TaskSnapshot.output`，完整后台输出仍由任务文件和限长的模型通知读取；TaskBackgrounded、TaskCompleted、ScheduledTaskCreated 的持久化/网关副本及 ScheduledTaskFired、MonitorEvent 的实时副本共享该会话预览 credit。要求持久化回执的完成事件先确认 Grow 写入，再发送实时展示。契约见 [工具桥 Grow 预算](../openspec/specs/session-timeline/spec.md#requirement-tool-bridge-grow-projections-share-preview-credits)。

重复 rewind 保留原始 branch leaf 和 response admission 来源，compaction 后也按未压缩前缀选择历史。普通 fork 先用父 Timeline 核对响应投影，再展开为子会话的普通继承展示行；父 request/attempt 身份不成为子 Timeline 的 admission。Resident reconnect 在初次回放遇到晚于 Timeline snapshot 的投影时，将物理读取截点留在该行之前，由 delta 连同后继更新交付；已合成过的响应继续去重。Exact projection/ACP 提交若只确认记录可读，仍须完成文件与目录同步才能 ACK，同步恢复后的重试不会重复追加。契约见 [rewind 来源](../openspec/specs/session-timeline/spec.md#requirement-repeated-rewind-preserves-retained-response-provenance)、[exact 持久确认](../openspec/specs/session-timeline/spec.md#requirement-exact-replay-commits-acknowledge-durable-barriers)、[fork 历史](../openspec/specs/client-surfaces/spec.md#requirement-forked-response-history-belongs-to-the-new-lineage) 和 [resident 回放截点](../openspec/specs/client-surfaces/spec.md#requirement-resident-replay-preserves-the-physical-snapshot-frontier)。

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

## 不改行为的最小变更

先创建 change，在生成的 `.openspec.yaml` 中保留 schema/created 并加入 `skip_specs: true`。proposal 写明不改行为的原因，design 简述影响，tasks 只列必要动作和验证。不创建空 delta，也不为工具维护虚构产品能力。完成后使用 `openspec archive <change> --skip-specs --yes`，再运行上述全量与归档校验。

Pager 列表布局缓存的每项几何查询以 `Option` 表达缓存范围外的索引；增量追加同时支持固定行高和变高布局。契约见 [列表布局缓存边界](../openspec/specs/client-surfaces/spec.md#requirement-list-layout-queries-respect-cached-item-bounds)。

Pager 子 Agent 权限审计组按 primary-turn epoch 聚合。成员追加与 reconnect 合并由 `ScrollbackState` 持有 epoch 并核对组的来源 epoch；`SubagentPermissionBlock` 不向 crate 外开放成员 mutation。契约见 [子 Agent 权限组 epoch](../openspec/specs/client-surfaces/spec.md#requirement-subagent-permission-groups-preserve-their-source-epoch)。

Minimal 每帧从当前 root、选中子 Agent 和 root 权限队列解析唯一可见 Agent；commit、viewport、live 和输入均使用该归属。切换视图会清空当前屏幕并重建新视图的原生提交边界，旧内容仍保留在终端历史中。实现入口为 `pager::minimal_api` 与 `pager-minimal::draw`；行为以 [Minimal 视图归属](../openspec/specs/client-surfaces/spec.md#requirement-minimal-frames-render-the-selected-agent-view) 为准。

## Rust 验证入口

按受影响 crate 缩小检查范围；下列命令是项目现有 CI/README 的入口，不意味着每次纯文档修改都运行全部测试。

```sh
cargo check --locked -p cli
cargo test --locked --lib -p chat-state -p sampling-types -p sampler -p memory -p workflow -p shell -p pager -p pager-minimal -- --test-threads=4
cargo build --locked -p cli --bin grow
```

PR 的 `core-regression` 在构建 `grow` 后串行运行 20 个指定 Pager PTY 回归，包括 minimal、设置、滚动选择、队列取消、真实 `less` transcript 恢复和 leader 多客户端共享会话。它们仍以 `#[ignore]` 保持普通本地 Cargo 套件轻量；CI 显式提供 `PAGER_BINARY` 并仅选择已登记的场景。完成历史 replay 的多客户端用例在附加 viewer 前等待第一轮 `turn_completed` 持久化记录；另一用例在首轮仍流式输出时附加 viewer，验证 resident reconnect 不会把运行中的 turn 当作冷恢复中断。单项可按工作流中的完整 target/name 运行：

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

跨会话协调与 Windows 存储还应检查对应 `.github/workflows/` 的平台回归。OpenSpec CI 只做文档格式与归档完成状态检查，语义由场景、源码、测试和 review 共同核对。

用量状态栏由 `ChatStateEvent::SessionUsageUpdated` 投影到账本变化时的 transient `SessionInfoUpdate.meta["grow/sessionUsage"]`，复用 `PromptUsage`，不增加周期查询或模型输入。主模型 attempt、子 Agent 终态结算、session incomplete 与冷恢复边界写入 Timeline；新 actor 按结算身份恢复 lifetime aggregate，resident reconnect 不新增分段。Pager 按累计值替换、丢弃倒退及历史 replay；normal 状态栏只显示 lifetime 总体，`/usage` 另外按 Initial run、Resume #N 展示分段。计费窗口和点击行为见 [会话用量契约](../openspec/specs/client-surfaces/spec.md#requirement-ordinary-agent-status-shows-session-usage)。

`UsageLedger` 同时保留账本所属 Agent 与每个已完成子 Agent 的身份分项；子 Agent 以稳定 `subagent_id` 沿既有终态 ACK fold 进入父账本并持久结算。`PromptUsage` 当前只投影包含全部 Agent 的总体和 provider/model 分项，不把 Agent 分项暴露到 `/usage`、headless 或 normal 状态栏。

Pager 的 cancel notification 没有响应体；`TurnCancelling` 因此在短窗口后复用 exact-prompt status 查询，以 terminal/unknown/error 收敛，Running 只重新起算窗口。首次 session load 使用带来源 surface 的临时 Agent：失败会删除临时页并返回 Welcome、原 Agent 或 Dashboard，原地 reload 仍走既有事务回滚。

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

异步文件 URL 和原始剪贴板文本是两个独立来源。分类未命中时，通过 `ClipboardPasteSource::file_url_text_on_miss` 判断是否需要保留 URL 文本；仅在成功探测且没有原始非空文本时使用。见 [异步 URL 回退契约](../openspec/specs/client-surfaces/spec.md#requirement-deferred-file-urls-survive-classification-miss)。

macOS `pbpaste -Prefer txt` 与图片 AppleScript 使用有界进程运行器：5 秒执行期限、stdout/stderr 各1 MiB，并回收所属进程组。图片 AppleScript 子进程继承单文件 50,000,000 字节 `RLIMIT_FSIZE`；图片写入私有临时文件后，Grow 最多读取 50,000,001 字节并拒绝超限结果。子进程文件限制不约束 helper/AppKit RSS、多个文件的合计用量或图片解码内存。简单图片粘贴会增加约 0.5–0.9 秒子进程与文件往返延迟。AppKit 在 Grow 内只用于读取 `changeCount`/`types` 元数据。见 [剪贴板图片读取契约](../openspec/specs/client-surfaces/spec.md#requirement-macos-clipboard-encoded-images-have-a-read-budget)、[子进程生命周期契约](../openspec/specs/client-surfaces/spec.md#requirement-clipboard-image-scripts-have-bounded-process-lifetimes) 与 [transfer file 写入预算](../openspec/changes/bound-macos-clipboard-transfer-file/specs/client-surfaces/spec.md)。

Kitty 非 PNG 转换在调用 sips 或 Rust 解码前检查源像素预算，见 [转换像素契约](../openspec/specs/client-surfaces/spec.md#requirement-kitty-image-conversion-bounds-source-pixels)。编码数据大小、源像素数、转换进程生命周期和终端直接解码是不同的资源边界。Grow 构造的 Kitty/iTerm2 图片上传 escape buffer 另有 100,000,000 字节序列化上限，见 [终端图片输出预算 change](../openspec/changes/bound-terminal-image-escape-output/specs/client-surfaces/spec.md)；它不限制终端进程解码或缓存图像的内存。

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

MCP 工具的模型路径把 `structuredContent` 与 text/resource 描述投成受既有文本限额约束的正文，image block 与 image resource 则作为运行时附件绕开文本截断，并在 Shell 结果结算时走原有正规化、图片预算与 Timeline follow-up。`use_tool` 的三层 JSON value 往返用 `TypedToolOutput.model_output` 携带附件，再还原到运行时 `MCPOutput`；默认 ACP `raw_output` 不序列化这些图片的 base64。显式 `expose_image_base64` 仍会把有界原始数据写进正文，长正文可能被截断并保存完整文件。direct `grow/mcp/call` 不写模型 Timeline，返回原生完整 `CallToolResult`；其 JSON 响应载荷上限为 16 MiB，超限报错而不返回残缺 JSON。契约见 [MCP 结果完整性](../openspec/specs/extension-runtime/spec.md#requirement-mcp-tool-results-retain-structured-and-typed-content) 和 [附件恢复](../openspec/specs/session-timeline/spec.md#requirement-mcp-image-evidence-survives-text-output-truncation)。

已批准的闲置接口清理边界以 [配置规则](../openspec/specs/configuration-rules/spec.md)、[工具协议](../openspec/specs/tool-authorization/spec.md)、[技能运行时](../openspec/specs/extension-runtime/spec.md) 和 [客户端设置持久化](../openspec/specs/client-surfaces/spec.md) 为准。技能列表由 SkillManager 管理；配置整份写入入口仍保留。

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

`GROW_PERF_ASSERT_HISTORY` 单独验证 load response 前历史文本的完整顺序，不用于性能对照。shell 的第一条通知可能是控制通知，不能当作第一条可见历史；完整 ACP load 时间也不能代替真实终端输入回显。normal Pager 加载期间的按键与提交队列由 `loading_replay_preserves_typed_prompt_until_session_loaded` 检查，终端恢复与后续提交由 ignored PTY `continue_resumes_session_with_history` 检查。

长会话的真实终端回显可用 ignored PTY 基准取样：先构建当前 `grow`，再运行 `PAGER_BINARY="$PWD/target/debug/grow" cargo test --locked -p pager --test pty_e2e_persistence resume_input_latency_128_512_turns -- --ignored --nocapture --test-threads=1`。它在隔离的 Grow home 和本地 mock provider 中准备 128、512 turns，报告 `--continue` 到末尾历史可见的时间及连续 20 次按键回显 p95。该合成样本不代表密集工具、多子会话、resident/cursor 或真实慢终端；这些场景需要分别取样，不能用单次结果宣称已满足全部长会话验收条件。

可见会话处于历史 replay 时，Pager 对 ACP、动画和周期性 UI maintenance 的自动绘制采用至少 100 ms 间隔；输入与显式动作维持即时绘制，`SessionLoaded` 后恢复正常间隔。状态维护和通知处理不受绘制节流影响；契约见 [冷恢复绘制节奏](../openspec/specs/client-surfaces/spec.md#requirement-cold-replay-paint-work-is-bounded-by-a-visible-progress-cadence)。

Pager 在恢复 batch 中保留已有布局，新增条目和 dirty 文本先使用估计高度，再由可见区域完成精确测量；隐藏 thinking 的跨条目间距回退到完整重建。turn 索引在最外层 batch 结束时重建，完整加载后的布局仍预热上方页面。加载中的导出与 transcript 保护继续以 [客户端契约](../openspec/specs/client-surfaces/spec.md) 为准。

JSONL full/light observation 仅从已验证 Timeline 派生内存 title/model；显式 writer restore 获得 lease 后才修复持久投影。观察不会因 Summary 滞后而争抢 writer lease，冲突和损坏仍拒绝，见 [只读观察契约](../openspec/specs/session-timeline/spec.md#requirement-session-observation-does-not-repair-durable-projections)。Workflow 从最新候选向前验证，最多保留 128 个有效 run，返回时保持 Timeline 顺序；无效候选不占名额。单文件读取预算保持，但最坏情况下会检查所有 Timeline 候选，见 [Workflow 恢复契约](../openspec/specs/workflow-execution/spec.md#requirement-workflow-restore-retains-the-latest-valid-runs)。

`grow/workflows/list` 将项目和用户目录扫描交给有界 blocking worker，五秒期限包含等待扫描资格；请求超时后无法中断的文件系统调用仍由 worker 持有许可直到退出，worker 错误与超时都返回 RPC 错误。见 [Workflow 列表执行边界](../openspec/specs/workflow-execution/spec.md#requirement-workflow-listing-has-a-bounded-execution-boundary)。

无 session 的非 chat `grow/commands/list` 将信任解析、插件配置与发现、技能及 Workflow 目录扫描放入同一共享单许可 worker，五秒期限覆盖排队和扫描；超时不代表后台扫描已停止，worker 退出前仍持有许可。Chat catalog 和 session 自有命令快照沿原路径返回。见 [pre-session 命令目录执行边界](../openspec/specs/client-surfaces/spec.md#requirement-pre-session-command-discovery-has-a-bounded-execution-boundary)。

Pager 的 `/config-agents` 先打开可关闭的 Loading 弹窗，再以单许可后台 worker 扫描 Agent 定义。五秒期限覆盖等待和扫描；超时只结束 UI 等待，阻塞线程退出前仍占用许可。扫描结果以弹窗加载 token 对账，关闭重开或外部编辑器刷新后，旧结果不能覆盖当前目录；成功切换启用状态只更新已写入的行。见 [Agents 弹窗发现契约](../openspec/specs/client-surfaces/spec.md#requirement-agent-configuration-modal-discovery-does-not-block-the-event-path)。

PromptWidget 创建和 `/agent` picker 打开时只装载静态内建 Agent，普通会话的项目、用户与插件 Agent 由同一 Pager 发现许可和五秒期限的后台任务补齐。结果核对可见 Agent、session binding 与 picker 请求；Workflow child 始终使用 Run 冻结快照，直接输入 `/agent <name>` 仍由 Shell 最终判定。见 [Agent 切换发现契约](../openspec/specs/client-surfaces/spec.md#requirement-switch-agent-discovery-does-not-block-pager-input)。

Messages 在单次请求内为中性工具 ID 建立映射，预留已有合法和 native ID，并同步编码调用与结果。原生 continuation 的 ID/签名保持，Timeline 不改写，见 [工具身份编码契约](../openspec/specs/model-sampling/spec.md#requirement-messages-tool-identity-encoding-preserves-distinct-exchanges)。64 字节是 Grow 的中性 ID 编码预算。

工具定义及模型图片资格 key 由 `tool-types` 持有，客户端身份值由 `config-types` 持有；HTTP 不引用 sampler/workspace 运行时。Pager 负责识别权限特殊行，renderer 只接收对应位置。版本策略由 `config::VersionPolicy` 持有，更新器的配置值和持久化操作由 CLI 组合层传入，仍复用 shell 的完整配置读写规则。Core regression 对这五条直接依赖边设有检查，不因此推断二进制或构建耗时的改善。

`bounded_replay_text_blocks_and_prompt_measurement` 使用 128/256 轮、每轮 8 个 512-byte chunk，经过真实 ACP handler 和 TextBlock，每 32 条通知准备布局并穿插输入。它分别报告通知、布局、输入处理、load 收尾和最终布局耗时，并检查历史完整性及输入队列；这是进程内阶段测量，不包含终端绘制、输入排队和设备回显。

```sh
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p pager --lib bounded_replay_text_blocks_and_prompt_measurement -- --ignored --nocapture --test-threads=1
```
