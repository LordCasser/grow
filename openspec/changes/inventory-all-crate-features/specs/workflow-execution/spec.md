## ADDED Requirements

### Requirement: Workflow metadata extraction evaluation
元数据提取 SHALL 先检查首段 let meta/const meta 文本前缀，编译整个脚本，再以 args=UNIT、100000 operations 上限求值；忽略求值错误并读取 scope.meta map。

#### Scenario: 后续宿主函数未注册
- **WHEN** meta 后调用 agent
- **THEN** 仍可提取已赋值的 meta；后续语法错误则在 compile 阶段失败。

#### Scenario: 声明不是纯静态对象
- **WHEN** 脚本在错误前修改 meta
- **THEN** 读取求值后的 map，不宣称这是只读取首条 AST 的静态提取器。

证据：`crates/codegen/workflow/src/meta.rs` — `extract_meta`。

### Requirement: Workflow metadata shape and byte limits
WorkflowMeta/PhaseMeta SHALL 拒绝未知字段；name/description/title trim 后非空，name 为小写 ASCII 字母数字及单 hyphen、无首尾 hyphen；长度按 UTF-8 字节校验。

#### Scenario: 字段限制
- **WHEN** name/description/when_to_use/title/detail 超过64/1024/2048/128/1024字节，或phase超过64项
- **THEN** 返回对应长度或数量错误。

#### Scenario: 重复phase
- **WHEN** 两个phase具有完全相同的原始title
- **THEN** 拒绝；去空白后相同但原始不同不由此集合判重，可选when_to_use/detail允许空串。

证据：`crates/codegen/workflow/src/meta.rs` — `validate_meta`。

### Requirement: Workflow typed host protocol
工作流 SHALL 通过 typed unbounded channel 向宿主发送 reserve/release、spawn、phase/log/diagnostic、budget、template、scratch read/write 和 Git diff 请求；结果请求使用 oneshot。

#### Scenario: Agent options与结果
- **WHEN** 反序列化 AgentOpts/AgentResult
- **THEN** 拒绝未知字段；opts支持prompt、label、model、agent_type、capability_mode、isolation_worktree、fork_context、resume_from、output_schema、phase；结果包含agent_id/success/output/cancelled/tokens_used/duration_ms。

#### Scenario: 职责边界
- **WHEN** 请求携带隔离或scratch参数
- **THEN** 本包只传递typed请求，真实权限、文件隔离及子Agent生命周期由宿主实现。

证据：`crates/codegen/workflow/src/host.rs` — `WorkflowHostRequest`。

### Requirement: Workflow engine limits and determinism guards
引擎 SHALL 应用调用方 max_ops、调用深度64、表达式深度128/64、字符串16MiB、数组和map各65536的限制，禁eval和模块解析，拒绝timestamp、sleep和无参exit。

#### Scenario: 取消纯计算
- **WHEN** CancellationToken已取消且触发Rhai progress
- **THEN** 返回Cancelled；blocking_recv期间不由该progress回调主动中断。

#### Scenario: 直接执行入口
- **WHEN** 直接调用run_workflow
- **THEN** 编译脚本并注入args，不自动执行extract_meta；自然返回也形成Completed。

证据：`crates/codegen/workflow/src/engine.rs` — `run_workflow`。

### Requirement: Workflow termination and value conversion
控制函数 SHALL 通过不可捕获的终止token表达Completed/Paused/AwaitingUser/BudgetExceeded/Cancelled/Failed；普通错误保留可捕获运行时错误语义。

#### Scenario: 嵌套函数终止
- **WHEN** 控制token包装在函数或module错误中
- **THEN** 递归提取token并保留对应outcome。

#### Scenario: 不可转换结果
- **WHEN** Dynamic不能转为JSON
- **THEN** dynamic_to_value回退null；complete()返回null，json_encode同样使用该转换。

证据：`crates/codegen/workflow/src/engine.rs` — `outcome_from_error`。

### Requirement: Workflow result call sequencing
结果调用 SHALL 分配从0递增的逻辑序号，上限10000；同序号的kind或请求hash不一致时fatal，已经完成的结果直接重放。

#### Scenario: 超过上限
- **WHEN** 脚本在10000个结果调用后继续调用
- **THEN** 不可catch的Failed，超限结果请求不发送；通知不占该序号。

#### Scenario: 新调用与pending
- **WHEN** 首次执行或恢复pending调用
- **THEN** 首次生成UUIDv4并先写intent再发送；恢复复用operation_id。

证据：`crates/codegen/workflow/src/engine.rs` — `host_call`。

### Requirement: Workflow host failure persistence
宿主 Unsupported/Failed SHALL 写入host error sentinel后作为可捕获错误返回，重放同样抛错；Budget/Cancelled终止并保留pending。

#### Scenario: 通道丢失
- **WHEN** send失败或oneshot reply被丢弃
- **THEN** fatal并保留已写intent。

#### Scenario: 结果层quota错误
- **WHEN** 结果reply为AgentCallQuotaExceeded
- **THEN** 返回可捕获错误而不complete；reserve阶段quota错误则BudgetExceeded。

证据：`crates/codegen/workflow/src/engine.rs` — `host_call`。

### Requirement: Workflow agent invocation and reservations
新agent调用 SHALL 先向宿主reserve一次额度；已有completed或pending不新增reserve；新调用Budget/Cancelled释放本轮额度。

#### Scenario: 选项重载
- **WHEN** 使用agent(prompt,map)或parallel的map项
- **THEN** 拒绝未知项和trim后空prompt，显式prompt覆盖map.prompt。

#### Scenario: 单字符串重载
- **WHEN** 调用agent(空字符串)
- **THEN** 本层直接构造AgentOpts而不执行map的非空检查，最终结果由宿主决定。

证据：`crates/codegen/workflow/src/engine.rs` — `spawn_agent_call`。

### Requirement: Workflow parallel dispatch and ordering
parallel SHALL 最多接受1024个options map，先校验全部选项与重放hash，再一次预留新项数量、逐项派发并按输入顺序收集结果。

#### Scenario: 非法输入
- **WHEN** 输入超量、非map、未知选项或空prompt
- **THEN** 校验阶段返回错误，不发送spawn。

#### Scenario: 乱序宿主完成
- **WHEN** 多个请求回复时机不同
- **THEN** 按输入顺序等待并构造结果数组；已有completed结果直接使用。

证据：`crates/codegen/workflow/src/engine.rs` — `register_host_fns`。

### Requirement: Workflow parallel terminal recovery
并行批次 SHALL 等待已收集的reply；任一Budget/Cancelled使本批保留pending、跳过全部live结果完成写入并释放本轮live_count。

#### Scenario: 部分成功且预算终止
- **WHEN** 一个agent成功，另一个BudgetExceeded
- **THEN** 成功回复也未写completed，恢复使用原operation_id再次请求宿主；效果去重依赖宿主。

#### Scenario: 普通失败
- **WHEN** 没有可恢复终态但有Failed/Unsupported/quota错误
- **THEN** 记录各live结果和error sentinel，重放不重启已完成兄弟；terminal marker优先于普通错误，多个marker保留先遇到者。

证据：`crates/codegen/workflow/src/engine.rs` — `resumable_terminal`。

### Requirement: Workflow notifications and replay flags
phase/log/print/debug/diagnostics_event SHALL 发送无回复通知，以当前seq是否被journal覆盖标注replayed，不持久化独立结果条目。

#### Scenario: 末尾通知
- **WHEN** 恢复时通知位于最后一个结果调用之后
- **THEN** replayed为false，即使整个脚本正在恢复。

#### Scenario: 通知接收端关闭
- **WHEN** channel发送失败
- **THEN** 忽略发送错误；diagnostics fields转换错误仍返回运行时错误。

证据：`crates/codegen/workflow/src/engine.rs` — `host_emit`。

### Requirement: Workflow pause and attention checkpoints
pause SHALL 每次终止而不记journal；await_user SHALL 首次记录null并返回AwaitingUser，相同检查点恢复时继续执行。

#### Scenario: 暂停类别
- **WHEN** 传入user/back_off/no_progress/verification/infra或parser别名backoff/blocked
- **THEN** 解析为对应PauseKind，非法kind报错；serde使用snake_case枚举值。

#### Scenario: 修改attention文本
- **WHEN** 恢复时修改kind原始字符串或message
- **THEN** 请求hash改变导致分歧，别名语义相同不免除hash检查。

证据：`crates/codegen/workflow/src/engine.rs` — `await_user`。

### Requirement: Workflow utility host calls and pure helpers
budget/template/scratch read/write/Git diff SHALL 采用结果journal重放；fingerprint采用稳定请求hash，json_encode产生JSON文本。

#### Scenario: 恢复未完成utility
- **WHEN** pending utility重新发送
- **THEN** 请求结构不传operation_id，不能由本包保证外部副作用只执行一次。

#### Scenario: 编码字符串
- **WHEN** 字符串包含换行或引号
- **THEN** JSON编码转义对应字符；预算重放返回记录时数值而非重新查询。

证据：`crates/codegen/workflow/src/engine.rs` — `git_diff_since`。

### Requirement: Workflow injected journal storage
Journal SHALL 支持memory与注入JournalStorage，生产接口只使用read_bounded/append/truncate，不拥有环境文件路径。

#### Scenario: 读取错误
- **WHEN** storage返回NotFound或InvalidData
- **THEN** 分别视为空journal或UnsafeRestore；其他IO错误传播。

#### Scenario: 存储界限
- **WHEN** load要求read_bounded提供最多64MiB
- **THEN** 依赖storage履行限制，不对返回Vec再次限长；cfg(test)文件实现的O_NOFOLLOW/sync_data不代表生产宿主保证。

证据：`crates/codegen/workflow/src/journal.rs` — `JournalStorage`。

### Requirement: Workflow journal line commit boundary
journal恢复 SHALL 只接纳换行终止的JSON行，跳过完整空白行，截断任何未终止尾部；JournalEntry拒绝未知字段。

#### Scenario: 合法但无换行
- **WHEN** 文件尾为完整JSON却没有newline
- **THEN** 不作为提交条目，truncate到尾部开始。

#### Scenario: 完整坏行
- **WHEN** newline结束的行不是合法JournalEntry
- **THEN** Parse并携带行号，不按torn tail丢弃。

证据：`crates/codegen/workflow/src/journal.rs` — `load_from_storage`。

### Requirement: Workflow journal logical folding and projection
恢复与只读project SHALL 要求逻辑条目从0稠密增长，最多10000项；带operation ID的同seq同kind/hash非pending物理行可替换逻辑结果。

#### Scenario: 普通重复行
- **WHEN** 普通非operation seq重复
- **THEN** Sequence错误；operation折叠保留ID，当前实现仍可接纳第二次非pending替换，不能宣称所有重复完成被拒绝。

#### Scenario: 只读投影
- **WHEN** project_physical_entry应用物理行
- **THEN** 不写storage、不更新bytes/last_line_start，不用于推导随后prune或写入安全性。

证据：`crates/codegen/workflow/src/journal.rs` — `fold_physical_entry`。

### Requirement: Workflow journal intent completion and caps
begin/complete/record SHALL 先追加物理行再推进内存；append包含newline并限制累计64MiB，memory也受限。

#### Scenario: 完成operation
- **WHEN** 匹配seq/kind/hash且现有结果仍pending
- **THEN** 追加完成物理行并替换逻辑结果；直接complete第二次拒绝。

#### Scenario: 存储失败或超限
- **WHEN** append失败或累计字节超限
- **THEN** 内存不推进；storage部分写入语义仍由宿主负责。record/begin的序号检查自身未施加10000条上限。

证据：`crates/codegen/workflow/src/journal.rs` — `complete_operation`。

### Requirement: Workflow journal replay and reservation projection
replay SHALL 校验seq/kind/hash，缺项返回None；普通replay拒绝pending，replay_operation返回Pending或Completed；reservation count统计所有spawn_agent逻辑条目。

#### Scenario: 有失败或pending agent
- **WHEN** 计算agent_reservation_count
- **THEN** 也计入这些条目，不仅统计成功结果。

#### Scenario: pending恢复
- **WHEN** replay_operation遇到持久intent
- **THEN** 返回原operation_id，不生成新ID。

证据：`crates/codegen/workflow/src/journal.rs` — `replay_operation`。

### Requirement: Workflow trailing host error pruning
prune SHALL 只在最后逻辑结果的非空host error被failure_detail包含时截断最后物理行；普通结果pop，operation恢复pending并保留ID。

#### Scenario: 错误不匹配
- **WHEN** 失败发生在别处或最后结果成功
- **THEN** 不prune。

#### Scenario: 已prune后再次请求
- **WHEN** 没有新的append或reload而再次需prune
- **THEN** last_line_start已清空，可返回无法定位偏移错误，不保证连续多次prune。

证据：`crates/codegen/workflow/src/journal.rs` — `prune_trailing_host_error`。

### Requirement: Workflow canonical request fingerprints
请求hash SHALL 对JSON对象递归排序、保留数组顺序，对kind加NUL及canonical JSON进行SHA256，取前16字节为32位十六进制。

#### Scenario: 对象键顺序变化
- **WHEN** 相同kind/payload仅对象插入顺序不同
- **THEN** hash一致；kind或数组顺序改变参与hash。

证据：`crates/codegen/workflow/src/journal.rs` — `request_hash`。

### Requirement: Workflow stub validation and authoring feedback
validate SHALL 先extract_meta再用memory journal、10000000 ops及stub宿主执行；默认agent预算128，可显式传入预算。

#### Scenario: 可接受outcome
- **WHEN** dry-run返回Completed/Paused/AwaitingUser
- **THEN** 成功报告name/phase数量及最多200字符内容摘要；Failed/Budget/Cancelled返回Run错误。

#### Scenario: stub边界
- **WHEN** 脚本调用agent/template/scratch/Git
- **THEN** 返回固定stub数据，不执行真实Agent、文件读写或Git；自定义args整体替代默认探针参数。

证据：`crates/codegen/workflow/src/validate.rs` — `validate_script_with_agent_budget`。

### Requirement: Workflow validation example exit contract
validate示例 SHALL 从stdin读取脚本并打印META/RUN结果，metadata失败退出1，dry-run失败退出2。

#### Scenario: 读取失败
- **WHEN** stdin read_to_string失败
- **THEN** expect panic；成功打印META OK与RUN OK。

#### Scenario: 作者错误
- **WHEN** 表达式过深、保留字或char字段访问错误
- **THEN** 相关meta/runtime校验错误可附带拆分+=、重命名或检查JSON/string类型提示。

证据：`crates/codegen/workflow/examples/validate.rs` — `main`。


### Requirement: Shell workflow list session launch gate

grow/workflows/list SHALL 要求sessionId并等待加载中的session handle；未知session返回错误。读取workflow catalog状态后仅launches_enabled决定返回该session cwd的workflow列表或空列表，management_available不参与该分支，不采用请求方任意cwd。

#### Scenario: Management available but launches disabled
- **WHEN** 目标session允许管理但不允许启动workflow
- **THEN** 返回空workflows列表。

证据：`crates/codegen/shell/src/extensions/skills.rs` — `pub async fn handle`；`crates/codegen/shell/src/extensions/skills.rs` — `struct WorkflowsListRequest`。


### Requirement: Shell Timeline owned workflow restore admission
会话Workflow恢复 SHALL 仅从Timeline的Workflow Spawned事件提取run_id，超过MAX_RESTORED_WORKFLOW_RUNS时丢弃最早事件，仅尝试保留的最近运行，不以目录扫描发现额外运行，也不在候选被跳过后回补旧运行。每个运行必须有lifecycle投影，否则整个恢复失败；run目录NotFound跳过，其他打开错误传播。cleared标记必须通过0字节上限读取，存在则跳过，非NotFound错误传播。manifest缺失或解码失败可交由resolve_workflow_restore_manifest以Timeline seed恢复；其他manifest读取错误只告警并跳过。解析出的manifest决定四位最小宽度的script_revision.rhai文件名，脚本须有界读取并为UTF-8、hash等于Timeline lifecycle.script_hash，args须有界读取并为JSON、hash等于lifecycle.args_hash；脚本目录/脚本/args读取或解码失败及hash不一致告警跳过。manifest解析协调或args hash计算返回的错误传播，已积累的恢复项不作为部分成功返回。

#### Scenario: Restore cap does not backfill
- **WHEN** 最近候选运行达到上限后，其中某个脚本缺失
- **THEN** 跳过该候选，不重新纳入已截掉的更早运行。

#### Scenario: Immutable inputs disagree with Timeline
- **WHEN** 脚本或args可读取但其hash不等于Timeline lifecycle记录
- **THEN** 告警并跳过该运行，不凭sidecar数据接纳它。

源码证据：
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `fn load_workflow_runs_sync`。


### Requirement: Shell workflow restore manifest authority and reconciliation
Workflow manifest选择 SHALL 首先验证run_id并解码Timeline initial_manifest；初始记录须匹配spawn的run_id/name/objective，使用当前manifest版本、script_revision与execution_epoch为0、Active状态、None handoff及固定workflows/run_id/journal.jsonl路径。初始记录经lifecycle协调后的副本必须通过恢复投影验证，否则返回错误，不能用sidecar覆盖错误的Timeline seed。sidecar仅在version/script_revision及run_id/name/objective/definition_id/definition_scope/definition_hash/runtime_route/phases/journal_path均与initial一致、revision不小于initial、execution_epoch不大于lifecycle，且协调副本有效时被选择；否则返回initial并标记used_timeline_seed。选择函数返回未修改的manifest，实际from_restored再次选择并协调state：open lifecycle转Interrupted、Completion handoff、process_interrupted消息；closed使用Timeline终态/handoff/message和epoch。缺失Timeline lifecycle或选择失败的运行告警跳过。修复过或回退seed的状态在source缓存锁释放后调用persist，排队失败仅告警，不取消返回的恢复状态；此处不证明持久化已完成。

#### Scenario: Invalid seed cannot be rescued by sidecar
- **WHEN** Timeline初始manifest不匹配spawn，但存在可解码sidecar
- **THEN** 返回错误，不选择sidecar绕过初始事实验证。

#### Scenario: Open execution is restored interrupted
- **WHEN** from_restored接纳的运行在Timeline中仍open
- **THEN** 协调为Interrupted和Completion handoff，消息为process_interrupted，并尝试持久化修复状态。

源码证据：
- `crates/codegen/shell/src/session/workflow/store.rs` — `pub(crate) fn resolve_workflow_restore_manifest`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn frozen_manifest_matches`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn reconcile_workflow_lifecycle`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `pub(crate) fn from_restored`。


- `crates/codegen/shell/src/session/workflow/store.rs` — `fn active_manifest_is_interrupted_when_no_live_executor_survives_restore`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn restore_uses_timeline_attention_handoff_instead_of_stale_manifest_projection`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn restore_uses_timeline_epoch_instead_of_stale_manifest_epoch`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn semantically_invalid_manifest_falls_back_without_hiding_other_runs`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn semantically_invalid_sidecar_rebuilds_from_timeline_initial_projection`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn frozen_sidecar_drift_falls_back_to_timeline_initial_projection`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn valid_sidecar_cannot_bypass_an_invalid_timeline_seed`。

### Requirement: Shell workflow source registration and persistence acknowledgement
WorkflowRunStore SHALL 验证run_id并拒绝sources缓存中已有的ID；有session_directory时先创建scripts目录，再检查pretty JSON args和script字节上限，依次写args.json、scripts/0000.rhai及script.rhai，全部成功后缓存revision=0的源。无session_directory时本函数不执行这些大小检查或文件写入，直接缓存。失败不在本函数回滚先前目录或文件写入；重复检查和最终insert使用分开的锁，不据此声称并发原子唯一注册。manifest_for仅从注册源获取script_revision并克隆state；未注册则initial_manifest/persist/persist_ack返回NotFound。initial_manifest及validate_persistable调用encoder检查可序列化大小，persist只发送WorkflowRunState，不等待落盘；persist_ack发送带oneshot回执的消息并等待actor结果，队列关闭或回执丢失返回BrokenPipe，本函数未设置等待超时。remove先删内存源再发送删除消息，发送失败仅告警，不恢复内存源。

#### Scenario: Queue acceptance is not durable acknowledgement
- **WHEN** persist成功向unbounded channel发送manifest
- **THEN** 仅证明消息被队列接纳，不证明actor已写入磁盘。

#### Scenario: Memory only registration
- **WHEN** store没有session_directory且输入超过文件模式大小限制
- **THEN** register本身不执行文件模式大小检查，仍可缓存源。

源码证据：
- `crates/codegen/shell/src/session/workflow/store.rs` — `pub(crate) fn register`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `pub(crate) fn initial_manifest`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `pub(crate) fn persist(`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `pub(crate) async fn persist_ack`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `pub(crate) fn remove`。


- `crates/codegen/shell/src/session/workflow/store.rs` — `fn script_and_args_are_immutable`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn register_rejects_symlinked_workflow_root`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn acknowledged_persist_returns_storage_failure`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn admission_uses_the_writer_manifest_size_limit`。

### Requirement: Shell workflow manifest codec and input hashing
Workflow manifest编码 SHALL 使用pretty JSON并检查MAX_WORKFLOW_MANIFEST_BYTES，超过返回InvalidInput；编码器不额外验证版本或恢复状态语义。两个decoder先要求version为u64且等于当前版本6，再按拒绝manifest未知字段的类型解码；非法或缺失版本及类型错误返回InvalidData，异版本使用session_version_mismatch。decoder本身不实施输入字节上限，由文件读取等调用方限额。脚本hash为原始UTF-8字节的BLAKE3十六进制，args hash对JSON对象递归按key排序、数组保持原顺序、标量不变，再对紧凑JSON序列化字节计算BLAKE3；不进行脚本空白归一化或数组排序。

#### Scenario: Version checked before body
- **WHEN** manifest版本不等于6且同时缺少当前版本要求字段
- **THEN** 返回版本不匹配，不先报告缺失字段。

#### Scenario: Script whitespace changes hash
- **WHEN** 脚本文本仅空白发生改变
- **THEN** hash按原始字节重新计算，不视为相同源。

源码证据：
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn encode_workflow_manifest`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `pub(crate) fn decode_workflow_manifest(`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `pub(crate) fn decode_workflow_manifest_value`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `pub(crate) fn workflow_script_hash`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `pub(crate) fn workflow_args_hash`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn canonicalize_json`。


- `crates/codegen/shell/src/session/workflow/store.rs` — `fn legacy_manifest_reports_version_before_decoding_the_v6_body`。

### Requirement: Shell workflow manifest revision serialization and tombstones
JSONL Workflow写入及删除 SHALL 先取得session writer lease、打开session，再spawn_blocking调用manifest写入或tombstone函数。Unix/Windows下二者使用state.lock排他锁，打开锁文件后开始5秒竞争期限，每次WouldBlock间隔10ms；这不是整个IO调用的超时。manifest写入验证run_id并打开或创建运行目录，锁内若零字节cleared标记存在则成功返回而不写manifest；标记读取非NotFound错误传播。已有state.json须通过有界读取及版本解码并匹配目录run_id；磁盘revision较大拒绝，revision相等且manifest完全相同成功不写，相等但内容不同拒绝，较小或文件缺失才编码并调用write_atomic写state.json。tombstone在同一锁内先原子写空cleared，再删除state.json，缺失state文件允许；删除失败不回滚已写标记，也不删除scripts/args等运行文件。非Unix/Windows路径返回Unsupported。actor普通写入及删除失败仅告警，带回执写入将storage结果发送给调用方，回执接收者消失不重试；成功结果可能是cleared或相同revision的无操作，不保证本次发生文件改写。

#### Scenario: Late state write after clear
- **WHEN** 运行已有合法空cleared标记，后续状态消息到达
- **THEN** 写入成功返回并保留清除状态，不重新生成state.json。

#### Scenario: Conflicting same revision
- **WHEN** 磁盘manifest和传入manifest的revision相同但内容不同
- **THEN** 拒绝写入并返回InvalidData。

#### Scenario: State removal fails after marker
- **WHEN** cleared写入成功后state.json删除失败
- **THEN** 返回错误，已写cleared保留，不回滚清除标记。

源码证据：
- `crates/codegen/shell/src/session/workflow/store.rs` — `pub(crate) fn write_workflow_run_manifest_in_directory`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `pub(crate) fn tombstone_workflow_run_in_directory`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn lock_workflow_state`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn write_workflow_run_state`。
- `crates/codegen/shell/src/session/storage/jsonl/mod.rs` — `async fn delete_workflow_run_state`。
- `crates/codegen/shell/src/session/persistence.rs` — `PersistenceMsg::WorkflowRunStateAndAck {`。

- `crates/codegen/shell/src/session/workflow/store.rs` — `fn stale_workflow_manifest_cannot_overwrite_newer_revision`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn equal_workflow_revision_is_idempotent_only_for_identical_content`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn concurrent_workflow_manifest_writes_converge_on_highest_revision`。
- `crates/codegen/shell/src/session/workflow/store.rs` — `fn workflow_tombstone_prevents_manifest_recreation`。
### Requirement: Pager workflow slash visibility and staged guide prompt

WorkflowCommand SHALL 名为workflow并接受可选参数，标记session_scoped同时offered_when_session_less=true，使dashboard可为下一primary session暂存；visible只由AppCtx.workflows_available决定。run始终返回SetBehaviorThenPrompt{mode:Workflow,prompt:Some(...)}：参数trim后非空则使用trim文本，空白时使用固定英文guide prompt，要求先询问自动化目标，再按name/description/when_to_use搜索，单一清晰匹配直接使用、近似匹配让用户选择、无匹配才引导创建session draft。命令不自行检查bundle中具体workflow、不创建draft，也不等待behavior切换完成。

#### Scenario: Dashboard stages workflow
- **WHEN** 当前无session但workflows_available为true
- **THEN** 命令仍可被offered并产生Workflow behavior加prompt的Action。

#### Scenario: Explicit workflow request
- **WHEN** 参数两端有空白但中间为用户prompt
- **THEN** 使用trim后的用户文本，不附加默认guide。

证据：`crates/codegen/pager/src/slash/commands/workflow.rs` — `WORKFLOW_GUIDE_PROMPT / WorkflowCommand::takes_args / session_scoped / offered_when_session_less / visible / run`。

### Requirement: Pager workflow run selector and host command routing

WorkflowRunCommand SHALL 接受参数、标记session_scoped，只在workflows_available且behavior为Workflow时visible，并以FullscreenOnly要求selector modal。run先trim参数：空串或大小写无关且无target的单词pause、resume、stop返回ToggleWorkflows；其他任意值，包括带target的管理动作与definition加args，返回HostCommand，command规范化为`/workflow-run `加trim后文本，description取命令自身描述。run不检查session、visibility、mode，也不解析definition/run身份；Shell拥有显式命令的再次gate和执行。

#### Scenario: Targetless management action
- **WHEN** 参数为` RESUME `
- **THEN** 大小写与空白规范化后打开selector，不向host发送无target resume。

#### Scenario: Named definition with arguments
- **WHEN** 参数为`deep-research compiler design`
- **THEN** 构造同文本的HostCommand，命令层不验证definition存在。

证据：`crates/codegen/pager/src/slash/commands/workflow_run.rs` — `WorkflowRunCommand::visible / mode_support / run`。

### Requirement: Pager loop slash provisional interval and scheduler injection

LoopCommand SHALL 接受必填参数并声明唯一required tool为共享SCHEDULER_CREATE_TOOL_NAME，不标记session_scoped。空白参数的run返回与tools共享loop_usage_message完全相同的Message，不声称默认10m。parse_loop_args先trim整体，仅当首个whitespace前token是可解析u64的非零ASCII数字加单字符s/m/h/d，且其后存在非空prompt时，提取该token；否则interval=None且整个trim输入为prompt，包括自然语言周期、0、溢出数字、坏suffix和单独`5m`。有效token只用于provisional human schedule的英文单双数格式；无token显示`scheduling…`。非空run返回InjectSkill：display_text以`/loop `拼原始未trim args，唯一Text block来自共享loop_schedule_instruction(raw args)，display_as_skill=false；preview使用解析后prompt、human schedule、next_fire_at=None、tag=loop。实际周期以模型调用scheduler_create后的权威更新为准，命令不创建任务或提供host默认。

#### Scenario: Compact explicit interval
- **WHEN** 参数为`30m check deploy status`
- **THEN** preview为every 30 minutes且prompt去掉首token，注入instruction仍接收原始参数。

#### Scenario: Bare interval token
- **WHEN** 参数只有`5m`
- **THEN** 不把它当无prompt周期，preview显示scheduling…且prompt为5m。

证据：`crates/codegen/pager/src/slash/commands/loop_cmd.rs` — `LOOP_REQUIRED_TOOLS / parse_loop_args / is_interval_token / interval_to_human / LoopCommand::required_tools / run`。
### Requirement: Pager scheduled task creation projection and provisional purge

A ScheduledTaskCreated update SHALL route by session match to the owning parent agent, remove every scheduled-task entry whose key begins `provisional-`, then upsert the canonical task id in the parent's root scheduled_tasks map. Existing entries replace prompt, human schedule and next fire while retaining created_at, tag and last_subagent_id; new entries use current Instant, tag loop and no last subagent. The return value is parent active status. Child matches still target the parent root map, and a matched-but-missing agent is treated as an invariant panic.

#### Scenario: Provisional cleanup
- **WHEN** a valid created update arrives
- **THEN** all provisional-prefixed entries are removed before upsert.

#### Scenario: Existing task
- **WHEN** the canonical id already exists
- **THEN** prompt, schedule and next fire are replaced without resetting other fields.

#### Scenario: New task
- **WHEN** the id is absent
- **THEN** a loop entry with current creation time and no last child is inserted.

#### Scenario: Child session
- **WHEN** the notification matches a child
- **THEN** the owning parent's scheduled task map is used.

#### Scenario: Visibility
- **WHEN** the owner is inactive
- **THEN** state updates but false is returned.

证据：`crates/codegen/pager/src/app/acp_handler/background.rs` — `handle_scheduled_task_created`。

### Requirement: Pager scheduled task fire update and unknown-task self-heal

A ScheduledTaskFired update SHALL route to the owning parent scheduled_tasks map. Existing task entries replace next_fire_at and record the firing subagent id while leaving prompt and schedule unchanged. An unknown task self-heals from the fire payload only when next_fire_at is present, creating a loop entry with current Instant and the firing child; when next_fire_at is absent it returns parent active status without insertion. Malformed, wrong-variant or unmatched notifications return false.

#### Scenario: Known fire
- **WHEN** task id exists
- **THEN** next fire and last subagent are updated.

#### Scenario: Recoverable unknown
- **WHEN** id is absent and next_fire_at is present
- **THEN** a new scheduled task is built from the fire payload.

#### Scenario: Terminal unknown
- **WHEN** id is absent and next_fire_at is absent
- **THEN** no entry is inserted.

#### Scenario: Inactive owner
- **WHEN** routing succeeds off-screen
- **THEN** state may update but false is returned.

证据：`crates/codegen/pager/src/app/acp_handler/background.rs` — `handle_scheduled_task_fired`。

### Requirement: Pager scheduled task deletion idempotent projection

A ScheduledTaskDeleted update SHALL route to the owning parent agent and remove the task id from its root scheduled_tasks map. It returns parent active status whether or not an entry existed, so true represents redraw ownership rather than mutation. Malformed, wrong-variant and unmatched updates return false; Child matches share the parent map.

#### Scenario: Existing
- **WHEN** the task id is present
- **THEN** it is removed and active status returned.

#### Scenario: Missing
- **WHEN** the id is already absent
- **THEN** the map stays unchanged but active status is still returned.

#### Scenario: Child
- **WHEN** the update matches a child session
- **THEN** the owning parent map is mutated.

#### Scenario: Invalid
- **WHEN** parse, variant or match fails
- **THEN** false is returned.

证据：`crates/codegen/pager/src/app/acp_handler/background.rs` — `handle_scheduled_task_deleted`。
### Requirement: Pager workflow command fingerprint capability and modal refetch

Workflow command fingerprinting SHALL include commands whose meta contains workflowSource, returning name, description, source as an optional string and optional workflowPath in catalog order; a nonstring workflowSource remains included with None. Workflow-run management availability is true exactly when any available command name equals `workflow-run` and is copied to every stored run. When an open modal's workflow fingerprint changes, refetch scheduling requires a current root session id and coalesces pending FetchWorkflowsList effects by AgentId before appending one effect.

#### Scenario: Fingerprint
- **WHEN** a command meta contains workflowSource
- **THEN** its display fields and optional path participate in ordered equality.

#### Scenario: Nonstring source
- **WHEN** the key exists but is not a string
- **THEN** the command is still included with absent source.

#### Scenario: Management
- **WHEN** workflow-run is present or absent
- **THEN** every run's management_available mirrors that presence.

#### Scenario: Refetch
- **WHEN** an agent has a session id and no pending per-agent fetch
- **THEN** one FetchWorkflowsList effect is queued.

#### Scenario: Coalesced
- **WHEN** that agent already has a pending fetch
- **THEN** no duplicate effect is added.

证据：`crates/codegen/pager/src/app/acp_handler/mod.rs` — `workflow_commands`、`refresh_workflow_run_capabilities`、`queue_open_workflows_modal_refresh`。
### Requirement: Pager workflow update direct ingestion outside Grow highwater

WorkflowUpdated SHALL delegate intact to AgentView ingestion. Root Grow live highwater neither rejects nor seeds workflow updates, while unexpected replay gating and reconnect cursor still apply. Payload revision/deduplication belongs to the ingestion method.

#### Scenario: Ingest
- **WHEN** WorkflowUpdated reaches dispatch
- **THEN** the full update and its boolean are delegated.

#### Scenario: Sequence
- **WHEN** its sequence is lower or higher than Grow highwater
- **THEN** this outer path neither rejects nor advances that highwater.

#### Scenario: Replay
- **WHEN** replay is unexpected
- **THEN** the shared replay gate may still drop it.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `WorkflowUpdated`。

### Requirement: Pager command bundle catalog and rewind execution effects

Slash-command execution SHALL send grow/commands/execute with command, description and invocation id and return the original request plus any ACP error. Catalog entry, bundle status and available-command refresh SHALL deserialize their typed or command-list payloads, with command refresh collapsing any failure to an empty vector. Rewind point discovery SHALL reject a non-null envelope error; rewind preview and execution SHALL both call grow/rewind/execute, differing by force false/true while preserving the requested mode, and shall reject an invalid typed response. This file does not prove command authorization or execution, bundle cache integrity, catalog source trust, rewind snapshot correctness, destructive rewind safety or stale-result rejection in the reducer.

#### Scenario: Slash command transport
- **WHEN** command execution succeeds or fails
- **THEN** SlashCommandExecuted retains agent, session, original request and an optional ACP error.

#### Scenario: Catalog envelope error
- **WHEN** bundle entry or status response includes error
- **THEN** the corresponding Failed result carries its string or an unknown-error fallback.

#### Scenario: Command refresh failure
- **WHEN** grow/commands/list fails or commands cannot deserialize
- **THEN** AvailableCommandsRefreshed carries an empty vector.

#### Scenario: Rewind preview
- **WHEN** preview is requested
- **THEN** grow/rewind/execute receives force false, target prompt index and mode and returns the typed response with the original target.

#### Scenario: Rewind execute
- **WHEN** execution is requested
- **THEN** the same method receives force true and a valid response becomes RewindExecuteComplete.

证据：`crates/codegen/pager/src/app/root/effects/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/workflow_run.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/workflow_run.rs SHALL 维护 session actor lifecycle and notifications 的入口 workflow_handoff_source_version, unresolved_workflow_handoff_identity, reconcile_restored_public_workflow_notifications, admit_public_workflow_handoff, named_workflow_snapshot, launch_named_workflow, manage_workflow_run, USAGE, workflow_workspace_report, parse_named_workflow_args, RunMatch, narrow_run_matches, run, exact_name_beats_prefix_of_uniquified_sibling, workflow_handoff_retry_identity_ignores_manifest_projection_revisions, restored_consumed_workflow_receipt_does_not_regenerate_payload, prefix_still_narrows_by_op_applicability, empty_selector_with_single_applicable_run_resolves (plus 3 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** workflow_handoff_source_version, unresolved_workflow_handoff_identity, reconcile_restored_public_workflow_notifications, admit_public_workflow_handoff, named_workflow_snapshot, launch_named_workflow, manage_workflow_run, USAGE, workflow_workspace_report, parse_named_workflow_args, RunMatch, narrow_run_matches, run, exact_name_beats_prefix_of_uniquified_sibling, workflow_handoff_retry_identity_ignores_manifest_projection_revisions, restored_consumed_workflow_receipt_does_not_regenerate_payload, prefix_still_narrows_by_op_applicability, empty_selector_with_single_applicable_run_resolves (plus 3 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** workflow_handoff_source_version, unresolved_workflow_handoff_identity, reconcile_restored_public_workflow_notifications, admit_public_workflow_handoff, named_workflow_snapshot, launch_named_workflow, manage_workflow_run, USAGE, workflow_workspace_report, parse_named_workflow_args, RunMatch, narrow_run_matches, run, exact_name_beats_prefix_of_uniquified_sibling, workflow_handoff_retry_identity_ignores_manifest_projection_revisions, restored_consumed_workflow_receipt_does_not_regenerate_payload, prefix_still_narrows_by_op_applicability, empty_selector_with_single_applicable_run_resolves (plus 3 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/actor/workflow_run.rs`。

### Requirement: Shell crates/codegen/shell/src/session/workflow/deep_research_tests.rs workflow definitions, runs, and journal contract

crates/codegen/shell/src/session/workflow/deep_research_tests.rs SHALL 维护 workflow definitions, runs, and journal 的入口 agent_result, finding, verdict, VerificationCase, DEFAULT_SYNTHESIS, run_scenario, str, evidence_levels_are_preserved_without_false_partial_status, malformed_verdict_ids_remain_local_to_the_affected_finding, adaptive_report_may_change_structure_and_omit_unused_sources, invalid_citations_or_unverified_media_use_detailed_fallback, all_deep_research_subagent_spawn_sites_request_full_capability_mode。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** agent_result, finding, verdict, VerificationCase, DEFAULT_SYNTHESIS, run_scenario, str, evidence_levels_are_preserved_without_false_partial_status, malformed_verdict_ids_remain_local_to_the_affected_finding, adaptive_report_may_change_structure_and_omit_unused_sources, invalid_citations_or_unverified_media_use_detailed_fallback, all_deep_research_subagent_spawn_sites_request_full_capability_mode 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** agent_result, finding, verdict, VerificationCase, DEFAULT_SYNTHESIS, run_scenario, str, evidence_levels_are_preserved_without_false_partial_status, malformed_verdict_ids_remain_local_to_the_affected_finding, adaptive_report_may_change_structure_and_omit_unused_sources, invalid_citations_or_unverified_media_use_detailed_fallback, all_deep_research_subagent_spawn_sites_request_full_capability_mode 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** agent_result, finding, verdict, VerificationCase, DEFAULT_SYNTHESIS, run_scenario, str, evidence_levels_are_preserved_without_false_partial_status, malformed_verdict_ids_remain_local_to_the_affected_finding, adaptive_report_may_change_structure_and_omit_unused_sources, invalid_citations_or_unverified_media_use_detailed_fallback, all_deep_research_subagent_spawn_sites_request_full_capability_mode 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/workflow/deep_research_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/workflow/host_service.rs workflow definitions, runs, and journal contract

crates/codegen/shell/src/session/workflow/host_service.rs SHALL 维护 workflow definitions, runs, and journal 的入口 WORKFLOW_MAX_AGENT_RUNS, WORKFLOW_MAX_SCRIPT_DIAGNOSTICS_EVENTS, WORKFLOW_MAX_SCRATCH_FILES, WORKFLOW_MAX_SCRATCH_FILE_BYTES, WORKFLOW_MAX_SCRATCH_TOTAL_BYTES, WORKFLOW_MAX_AGENT_PROMPT_BYTES, WORKFLOW_MAX_TEMPLATE_OUTPUT_BYTES, WORKFLOW_MAX_PHASE_BYTES, WORKFLOW_MAX_LOG_BYTES, WORKFLOW_CHILD_DRAIN_TIMEOUT, WORKFLOW_MAX_SCRATCH_NAME_BYTES, SCRATCH_ARTIFACT_ROOT, DiagnosticHook, WorkflowHostParams, HostDrainOutcome, spawn_workflow_host_service, reply_cancelled, HostService (plus 22 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** WORKFLOW_MAX_AGENT_RUNS, WORKFLOW_MAX_SCRIPT_DIAGNOSTICS_EVENTS, WORKFLOW_MAX_SCRATCH_FILES, WORKFLOW_MAX_SCRATCH_FILE_BYTES, WORKFLOW_MAX_SCRATCH_TOTAL_BYTES, WORKFLOW_MAX_AGENT_PROMPT_BYTES, WORKFLOW_MAX_TEMPLATE_OUTPUT_BYTES, WORKFLOW_MAX_PHASE_BYTES, WORKFLOW_MAX_LOG_BYTES, WORKFLOW_CHILD_DRAIN_TIMEOUT, WORKFLOW_MAX_SCRATCH_NAME_BYTES, SCRATCH_ARTIFACT_ROOT, DiagnosticHook, WorkflowHostParams, HostDrainOutcome, spawn_workflow_host_service, reply_cancelled, HostService (plus 22 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** WORKFLOW_MAX_AGENT_RUNS, WORKFLOW_MAX_SCRIPT_DIAGNOSTICS_EVENTS, WORKFLOW_MAX_SCRATCH_FILES, WORKFLOW_MAX_SCRATCH_FILE_BYTES, WORKFLOW_MAX_SCRATCH_TOTAL_BYTES, WORKFLOW_MAX_AGENT_PROMPT_BYTES, WORKFLOW_MAX_TEMPLATE_OUTPUT_BYTES, WORKFLOW_MAX_PHASE_BYTES, WORKFLOW_MAX_LOG_BYTES, WORKFLOW_CHILD_DRAIN_TIMEOUT, WORKFLOW_MAX_SCRATCH_NAME_BYTES, SCRATCH_ARTIFACT_ROOT, DiagnosticHook, WorkflowHostParams, HostDrainOutcome, spawn_workflow_host_service, reply_cancelled, HostService (plus 22 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** WORKFLOW_MAX_AGENT_RUNS, WORKFLOW_MAX_SCRIPT_DIAGNOSTICS_EVENTS, WORKFLOW_MAX_SCRATCH_FILES, WORKFLOW_MAX_SCRATCH_FILE_BYTES, WORKFLOW_MAX_SCRATCH_TOTAL_BYTES, WORKFLOW_MAX_AGENT_PROMPT_BYTES, WORKFLOW_MAX_TEMPLATE_OUTPUT_BYTES, WORKFLOW_MAX_PHASE_BYTES, WORKFLOW_MAX_LOG_BYTES, WORKFLOW_CHILD_DRAIN_TIMEOUT, WORKFLOW_MAX_SCRATCH_NAME_BYTES, SCRATCH_ARTIFACT_ROOT, DiagnosticHook, WorkflowHostParams, HostDrainOutcome, spawn_workflow_host_service, reply_cancelled, HostService (plus 22 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/workflow/host_service.rs`。

### Requirement: Shell crates/codegen/shell/src/session/workflow/manager.rs workflow definitions, runs, and journal contract

crates/codegen/shell/src/session/workflow/manager.rs SHALL 维护 workflow definitions, runs, and journal 的入口 WORKFLOW_MAX_ACTIVE_RUNS_PER_SESSION, WORKFLOW_DEFAULT_AGENT_BUDGET, ActiveRun, SessionJournalStorage, read_bounded, append, truncate, LaunchSpec, LaunchError, WorkflowManager, journal_storage, new, close_admission, admission_snapshot, ensure_open_for_ingress, check_admission, rollback_unspawned_run, set_next_run_route (plus 58 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** WORKFLOW_MAX_ACTIVE_RUNS_PER_SESSION, WORKFLOW_DEFAULT_AGENT_BUDGET, ActiveRun, SessionJournalStorage, read_bounded, append, truncate, LaunchSpec, LaunchError, WorkflowManager, journal_storage, new, close_admission, admission_snapshot, ensure_open_for_ingress, check_admission, rollback_unspawned_run, set_next_run_route (plus 58 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** WORKFLOW_MAX_ACTIVE_RUNS_PER_SESSION, WORKFLOW_DEFAULT_AGENT_BUDGET, ActiveRun, SessionJournalStorage, read_bounded, append, truncate, LaunchSpec, LaunchError, WorkflowManager, journal_storage, new, close_admission, admission_snapshot, ensure_open_for_ingress, check_admission, rollback_unspawned_run, set_next_run_route (plus 58 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** WORKFLOW_MAX_ACTIVE_RUNS_PER_SESSION, WORKFLOW_DEFAULT_AGENT_BUDGET, ActiveRun, SessionJournalStorage, read_bounded, append, truncate, LaunchSpec, LaunchError, WorkflowManager, journal_storage, new, close_admission, admission_snapshot, ensure_open_for_ingress, check_admission, rollback_unspawned_run, set_next_run_route (plus 58 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/workflow/manager.rs`。

### Requirement: Shell crates/codegen/shell/src/session/workflow/mod.rs workflow definitions, runs, and journal contract

crates/codegen/shell/src/session/workflow/mod.rs SHALL 维护 workflow definitions, runs, and journal 的入口 extracted_deep_research_uses_the_user_workflow_registry。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Durable boundary
- **WHEN** extracted_deep_research_uses_the_user_workflow_registry 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/workflow/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/session/workflow/notify.rs workflow definitions, runs, and journal contract

crates/codegen/shell/src/session/workflow/notify.rs SHALL 维护 workflow definitions, runs, and journal 的入口 WorkflowNotifySender, new, emit, emit_ephemeral, broadcast, dispatch, build_workflow_updated, phase_states_derive_from_current, completed_run_marks_current_phase_done。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、channel or acknowledgement flow、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** WorkflowNotifySender, new, emit, emit_ephemeral, broadcast, dispatch, build_workflow_updated, phase_states_derive_from_current, completed_run_marks_current_phase_done 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** WorkflowNotifySender, new, emit, emit_ephemeral, broadcast, dispatch, build_workflow_updated, phase_states_derive_from_current, completed_run_marks_current_phase_done 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/workflow/notify.rs`。

### Requirement: Shell crates/codegen/shell/src/session/workflow/registry.rs workflow definitions, runs, and journal contract

crates/codegen/shell/src/session/workflow/registry.rs SHALL 维护 workflow definitions, runs, and journal 的入口 MAX_WORKFLOW_SOURCE_BYTES, MAX_WORKFLOW_NAME_BYTES, ResolvedWorkflow, WorkflowSource, ResolveError, str, project_root, user_workflow_dir, WorkflowRegistry, RegistryEntry, scan, scan_with_user_root, resolve_by_name, resolve_by_id, resolve_entry, list, diagnostics, resolved_entry (plus 34 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、session/timeline state projection、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** MAX_WORKFLOW_SOURCE_BYTES, MAX_WORKFLOW_NAME_BYTES, ResolvedWorkflow, WorkflowSource, ResolveError, str, project_root, user_workflow_dir, WorkflowRegistry, RegistryEntry, scan, scan_with_user_root, resolve_by_name, resolve_by_id, resolve_entry, list, diagnostics, resolved_entry (plus 34 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** MAX_WORKFLOW_SOURCE_BYTES, MAX_WORKFLOW_NAME_BYTES, ResolvedWorkflow, WorkflowSource, ResolveError, str, project_root, user_workflow_dir, WorkflowRegistry, RegistryEntry, scan, scan_with_user_root, resolve_by_name, resolve_by_id, resolve_entry, list, diagnostics, resolved_entry (plus 34 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/workflow/registry.rs`。

### Requirement: Shell crates/codegen/shell/src/session/workflow/schema_contract.rs workflow definitions, runs, and journal contract

crates/codegen/shell/src/session/workflow/schema_contract.rs SHALL 维护 workflow definitions, runs, and journal 的入口 SCHEMA_CONTRACT_RETRIES, contract_prompt, CONTRACT_OUTPUT_MAX_BYTES, compile_contract_schema, validate_contract_output, v, fenced_json_after_prose_validates, bare_json_validates, json_embedded_in_prose_validates, last_fence_wins, schema_violation_reports_schema_error, no_json_reports_parse_error, external_references_are_rejected, unsupported_backtracking_regex_is_rejected。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** SCHEMA_CONTRACT_RETRIES, contract_prompt, CONTRACT_OUTPUT_MAX_BYTES, compile_contract_schema, validate_contract_output, v, fenced_json_after_prose_validates, bare_json_validates, json_embedded_in_prose_validates, last_fence_wins, schema_violation_reports_schema_error, no_json_reports_parse_error, external_references_are_rejected, unsupported_backtracking_regex_is_rejected 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/workflow/schema_contract.rs`。

### Requirement: Shell crates/codegen/shell/src/session/workflow/tracker.rs workflow definitions, runs, and journal contract

crates/codegen/shell/src/session/workflow/tracker.rs SHALL 维护 workflow definitions, runs, and journal 的入口 WorkflowRunStatus, as_str, str, is_terminal, is_paused, is_resumable, to_timeline, from_timeline, from_pause, WORKFLOW_PAUSE_MESSAGE_MAX_BYTES, capped_pause_message, default_label_for, WorkflowAgentRow, WORKFLOW_AGENT_ROWS_MAX, WorkflowSamplerSnapshot, from_sampler, with_reasoning_efforts, with_auto_compact_threshold_percent (plus 120 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** WorkflowRunStatus, as_str, str, is_terminal, is_paused, is_resumable, to_timeline, from_timeline, from_pause, WORKFLOW_PAUSE_MESSAGE_MAX_BYTES, capped_pause_message, default_label_for, WorkflowAgentRow, WORKFLOW_AGENT_ROWS_MAX, WorkflowSamplerSnapshot, from_sampler, with_reasoning_efforts, with_auto_compact_threshold_percent (plus 120 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** WorkflowRunStatus, as_str, str, is_terminal, is_paused, is_resumable, to_timeline, from_timeline, from_pause, WORKFLOW_PAUSE_MESSAGE_MAX_BYTES, capped_pause_message, default_label_for, WorkflowAgentRow, WORKFLOW_AGENT_ROWS_MAX, WorkflowSamplerSnapshot, from_sampler, with_reasoning_efforts, with_auto_compact_threshold_percent (plus 120 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/workflow/tracker.rs`。

### Requirement: Shell crates/codegen/shell/src/session/workflow/workspace.rs workflow definitions, runs, and journal contract

crates/codegen/shell/src/session/workflow/workspace.rs SHALL 维护 workflow definitions, runs, and journal 的入口 WORKSPACE_VERSION, MAX_WORKSPACE_STATE_BYTES, WorkspaceError, DraftSource, DraftRecord, PendingPublish, WorkspaceState, default, WorkflowDefinition, WorkflowCatalog, WorkflowWorkspace, open, open_observational_in_session, open_for_update_in_session, open_reconciled_in_session, catalog_in_session_observational, search_in_session_observational, focus_in_session (plus 46 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、session/timeline state projection、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** WORKSPACE_VERSION, MAX_WORKSPACE_STATE_BYTES, WorkspaceError, DraftSource, DraftRecord, PendingPublish, WorkspaceState, default, WorkflowDefinition, WorkflowCatalog, WorkflowWorkspace, open, open_observational_in_session, open_for_update_in_session, open_reconciled_in_session, catalog_in_session_observational, search_in_session_observational, focus_in_session (plus 46 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** WORKSPACE_VERSION, MAX_WORKSPACE_STATE_BYTES, WorkspaceError, DraftSource, DraftRecord, PendingPublish, WorkspaceState, default, WorkflowDefinition, WorkflowCatalog, WorkflowWorkspace, open, open_observational_in_session, open_for_update_in_session, open_reconciled_in_session, catalog_in_session_observational, search_in_session_observational, focus_in_session (plus 46 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/workflow/workspace.rs`。
### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs durable scheduled task runtime contract
crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs SHALL implement the durable scheduled task runtime boundary through validate schedule identity/intervals, persist generation and occurrence journal state, and report bounded create/list/delete outcomes. Its source symbols MAX_SCHEDULED_TASKS, DURABILITY_BARRIER_TIMEOUT, LoopFireOutcome, ExpiryPersistenceOutcome, PendingDurableRemoval, truncate_chars, task_created_payload, task_removed_payload, log_rollover, str, SchedulerActor, await_bounded, persist_resources, await_persistence_outcome, publish_durable_removal, acknowledge_and_clear_one_shot, rollback_unadmitted_one_shot, reconcile_one_shot_occurrences (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `MAX_SCHEDULED_TASKS`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `DURABILITY_BARRIER_TIMEOUT`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `LoopFireOutcome`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `ExpiryPersistenceOutcome`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `PendingDurableRemoval`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `truncate_chars`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `task_created_payload`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `task_removed_payload`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `log_rollover`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `SchedulerActor`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `await_bounded`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `persist_resources`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `await_persistence_outcome`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `publish_durable_removal`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `acknowledge_and_clear_one_shot`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `rollback_unadmitted_one_shot`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `reconcile_one_shot_occurrences`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `complete_pending_removal`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `await_subagent_wiring_for_due_tasks`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `STARTUP_WIRING_GRACE`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `POLL_INTERVAL`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `compute_next_fire_delay`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `fire_next_task`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `fire_as_loop_subagent`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `announce_existing_tasks`；`crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs` — `handle_command`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs durable scheduled task runtime contract
crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs SHALL implement the durable scheduled task runtime boundary through validate schedule identity/intervals, persist generation and occurrence journal state, and report bounded create/list/delete outcomes. Its source symbols SchedulerCreateInput, SchedulerCreateOutput, SchedulerCreateTool, kind, tool_namespace, description_template, emitted_notifications, str, requires_expr, Args, Output, id, description, capabilities, run, scheduler_resources, input, task_count (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、platform/feature conditional、sandbox, trust, or allow/deny policy; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `SchedulerCreateInput`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `SchedulerCreateOutput`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `SchedulerCreateTool`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `emitted_notifications`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `scheduler_resources`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `input`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `task_count`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `create_requires_interval_and_prompt`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `removed_recurring_input_is_rejected`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `update_unknown_task_id_errors_and_never_creates`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `update_with_no_patch_fields_errors`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `create_then_update_patches_in_place`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `schema_advertises_task_id_and_has_no_recurring_input`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `loop_usage_message_has_no_host_default`；`crates/codegen/tools/src/implementations/grow_build/scheduler/create.rs` — `loop_schedule_instruction_holds_invariants`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs durable scheduled task runtime contract
crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs SHALL implement the durable scheduled task runtime boundary through validate schedule identity/intervals, persist generation and occurrence journal state, and report bounded create/list/delete outcomes. Its source symbols SCHEDULER_DELETE_TOOL_NAME, SchedulerDeleteInput, SchedulerDeleteOutput, SchedulerDeleteTool, kind, tool_namespace, description_template, emitted_notifications, str, requires_expr, Args, Output, id, description, capabilities, run follow explicit markers serde/json wire or configuration、explicit error classification、channel, fanout, or acknowledgement flow、child process execution、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、scheduler generation/journal state; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `SCHEDULER_DELETE_TOOL_NAME`；`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `SchedulerDeleteInput`；`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `SchedulerDeleteOutput`；`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `SchedulerDeleteTool`；`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `emitted_notifications`；`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build/scheduler/delete.rs` — `run`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs durable scheduled task runtime contract
crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs SHALL implement the durable scheduled task runtime boundary through validate schedule identity/intervals, persist generation and occurrence journal state, and report bounded create/list/delete outcomes. Its source symbols MINIMUM_INTERVAL_SECS, parse_interval, interval_to_human, parse_minutes, parse_hours, parse_days, parse_seconds_clamped_to_minimum, parse_empty_returns_error, parse_invalid_format_returns_error, parse_zero_returns_error, parse_overflow_returns_error, human_readable_minutes, human_readable_hours, human_readable_days, human_readable_seconds, parse_with_whitespace follow explicit markers explicit error classification、platform/feature conditional、session, prompt, goal, or subagent context、repository/worktree scope、scheduler generation/journal state; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `MINIMUM_INTERVAL_SECS`；`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `parse_interval`；`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `interval_to_human`；`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `parse_minutes`；`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `parse_hours`；`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `parse_days`；`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `parse_seconds_clamped_to_minimum`；`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `parse_empty_returns_error`；`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `parse_invalid_format_returns_error`；`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `parse_zero_returns_error`；`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `parse_overflow_returns_error`；`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `human_readable_minutes`；`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `human_readable_hours`；`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `human_readable_days`；`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `human_readable_seconds`；`crates/codegen/tools/src/implementations/grow_build/scheduler/interval.rs` — `parse_with_whitespace`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs durable scheduled task runtime contract
crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs SHALL implement the durable scheduled task runtime boundary through validate schedule identity/intervals, persist generation and occurrence journal state, and report bounded create/list/delete outcomes. Its source symbols SchedulerListInput, ScheduledTaskSummary, SchedulerListOutput, SchedulerListTool, kind, tool_namespace, description_template, requires_expr, Args, Output, id, description, capabilities, run follow explicit markers serde/json wire or configuration、explicit error classification、channel, fanout, or acknowledgement flow、child process execution、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、scheduler generation/journal state; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs` — `SchedulerListInput`；`crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs` — `ScheduledTaskSummary`；`crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs` — `SchedulerListOutput`；`crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs` — `SchedulerListTool`；`crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build/scheduler/list.rs` — `run`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/scheduler/mod.rs durable scheduled task runtime contract
crates/codegen/tools/src/implementations/grow_build/scheduler/mod.rs SHALL implement the durable scheduled task runtime boundary through validate schedule identity/intervals, persist generation and occurrence journal state, and report bounded create/list/delete outcomes. Its source symbols mod follow explicit markers scheduler generation/journal state; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/grow_build/scheduler/mod.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/grow_build/scheduler/mod.rs` — `mod`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs durable scheduled task runtime contract
crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs SHALL implement the durable scheduled task runtime boundary through validate schedule identity/intervals, persist generation and occurrence journal state, and report bounded create/list/delete outcomes. Its source symbols MAX_PENDING_ONE_SHOTS, MAX_QUARANTINED_TASK_IDS, MAX_TASK_ID_BYTES, ScheduledOccurrenceId, new, deserialize, ScheduledOccurrenceVersions, try_new, fire, removal, contains, PersistedVersions, OneShotOccurrence, occurrence_id, task_id, removal_version, PersistedOccurrence, OccurrenceJournal (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、channel, fanout, or acknowledgement flow、platform/feature conditional、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle、scheduler generation/journal state; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `MAX_PENDING_ONE_SHOTS`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `MAX_QUARANTINED_TASK_IDS`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `MAX_TASK_ID_BYTES`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `ScheduledOccurrenceId`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `new`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `deserialize`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `ScheduledOccurrenceVersions`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `try_new`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `fire`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `removal`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `contains`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `PersistedVersions`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `OneShotOccurrence`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `occurrence_id`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `task_id`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `removal_version`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `PersistedOccurrence`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `OccurrenceJournal`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `is_empty`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `quarantine_diagnostics`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `decode_json`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `quarantine_task_id`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `parse_json_array`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `parse_required_json_array`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `parse_json_bool`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `quarantined_task_id`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `OccurrenceJournalError`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal.rs` — `OneShotJournalConflict`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs durable scheduled task runtime contract
crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs SHALL implement the durable scheduled task runtime boundary through validate schedule identity/intervals, persist generation and occurrence journal state, and report bounded create/list/delete outcomes. Its source symbols GENERATION, uuid, task, version, versions, occurrence_json, valid_occurrence_json, journal, state, prepare, prepare_finish_and_mutation_failures_preserve_state, validation_rejects_impossible_versions_and_non_rfc_identity, exactly_fifty_round_trips_and_mutation_reports_journal_full, array_shaped_journal_is_not_loaded_as_current_state, overflow_tail_suppresses_globally_and_never_serializes_a_fifty_first_entry, malformed_missing_task_identity_blocks_all_one_shots_across_reload, inconsistent_current_overflow_metadata_normalizes_and_round_trips, production_loader_preserves_tasks_and_quarantine_metadata (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、channel, fanout, or acknowledgement flow、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle、scheduler generation/journal state; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `GENERATION`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `uuid`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `task`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `version`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `versions`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `occurrence_json`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `valid_occurrence_json`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `journal`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `state`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `prepare`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `prepare_finish_and_mutation_failures_preserve_state`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `validation_rejects_impossible_versions_and_non_rfc_identity`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `exactly_fifty_round_trips_and_mutation_reports_journal_full`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `array_shaped_journal_is_not_loaded_as_current_state`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `overflow_tail_suppresses_globally_and_never_serializes_a_fifty_first_entry`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `malformed_missing_task_identity_blocks_all_one_shots_across_reload`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `inconsistent_current_overflow_metadata_normalizes_and_round_trips`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `production_loader_preserves_tasks_and_quarantine_metadata`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `reconciliation_exposes_only_persistence_and_suppression_foundation`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `conflict_receipts_produce_diagnostics_and_suppress_every_task`；`crates/codegen/tools/src/implementations/grow_build/scheduler/occurrence_journal_tests.rs` — `empty_journal_omits_optional_field`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs durable scheduled task runtime contract
crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs SHALL implement the durable scheduled task runtime boundary through validate schedule identity/intervals, persist generation and occurrence journal state, and report bounded create/list/delete outcomes. Its source symbols SchedulerVersion, generation, revision, generation_id, from_parts, SchedulerClock, SchedulerReservation, GenerationRollover, SchedulerCommit, version_at, commit_next, new, snapshot, prepare_transition, at_revision_for_test, default, SchedulerError, scheduler_tool_error (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、channel, fanout, or acknowledgement flow、timeout, budget, or rate limit、platform/feature conditional、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `SchedulerVersion`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `generation`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `revision`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `generation_id`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `from_parts`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `SchedulerClock`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `SchedulerReservation`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `GenerationRollover`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `SchedulerCommit`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `version_at`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `commit_next`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `new`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `snapshot`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `prepare_transition`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `at_revision_for_test`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `default`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `SchedulerError`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `scheduler_tool_error`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `ScheduledTask`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `LOOP_FRESH_CHAIN_EVERY`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `LOOP_COMPLETION_OUTPUT_CAP`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `MAX_SCHEDULER_TRANSITIONS`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `with_fire_immediately`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `next_fire_at`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `is_expired`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `SchedulerState`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `SchedulerSnapshot`；`crates/codegen/tools/src/implementations/grow_build/scheduler/types.rs` — `SchedulerHandle`。
### Requirement: Pager agent input test: workflow_child_agent_picker_uses_only_run_snapshot_names
A workflow child Agent picker SHALL expose only immutable run-snapshot names and mark them as workflow entries.

#### Scenario: Workflow Agent picker
- **WHEN** workflow_agent_names is present and the agent picker opens
- **THEN** the picker contains exactly the snapshot names with workflow labeling.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `workflow_child_agent_picker_uses_only_run_snapshot_names`。
