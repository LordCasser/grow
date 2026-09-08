# workflow 逐文件审阅（进行中）

包保持 pending；未完成 engine、validate 与 example 阅读，不计入完整覆盖。

## 已阅读范围

Cargo.toml、src/lib.rs、src/host.rs、src/run.rs、src/meta.rs，以及 src/journal.rs 全部 1067 行（含测试）。engine.rs、validate.rs、examples/validate.rs 待补。

## journal 源码事实

- JournalStorage 由宿主注入 read_bounded/append/truncate；生产引擎没有环境文件路径。文件、symlink、O_NOFOLLOW 和 sync_data 实现位于 cfg(test)，不能从此宣称生产存储有相同隔离保证。
- restore 请求存储提供不超过 64 MiB 内容；NotFound 作为空 journal，InvalidData 转 UnsafeRestore。调用方依赖 read_bounded 实现履行界限，load 本身不再次检查返回 Vec 长度。
- JSONL 只有换行结束才算提交；最后无换行的片段即使是合法 JSON 也 truncate。完整空白行跳过，完整非法 JSON 报带行号的 Parse。JournalEntry 拒绝未知字段。
- 逻辑序号从 0 稠密增长；restore/project 最多 10000 逻辑条目。record/begin 的 validate_sequence 本身没有此数量限制，执行层限制待核对。
- 普通 replay 校验 seq/kind/request hash，缺条目返回 None；未完成 operation 不能作为普通结果重放。operation replay 区分 Pending（保存 ID）和 Completed。
- begin 先持久化 pending sentinel 再推进内存；complete 校验 kind/hash、存在 operation ID 且当前仍 pending，追加同序号物理结果后替换逻辑条目。持久化失败不推进内存，但外部 append 是否发生部分写取决于 storage。
- restore/project 的 fold 对带 operation ID 的旧序号接受同 kind/hash 的非 pending 替换；该分支未检查旧 result 是否仍 pending，因此操作完成后仍可能接受第二个完成物理行。已有 duplicate 测试只覆盖没有 operation ID 的普通 log，不能据此证明所有重复完成均拒绝。
- projector 只更新 entries/operation_ids，不写 storage，也不更新 bytes/last_line_start；适合只读投影，不能推断可接着安全执行写入/prune。
- 每次 append 包含换行并受 64 MiB 上限约束，memory journal 同样受限；超过返回 Full，持久化先于内存推进。
- prune 仅当最后逻辑结果包含非空 host error 字符串且 failure_detail 包含该字符串才处理。按最后物理行偏移 truncate，普通条目 pop；operation 恢复为 pending sentinel、保留 ID。之后清空 last_line_start，再次 prune 不保证可执行。
- parallel 操作完成物理顺序与逻辑顺序可能不同，prune 使用最后逻辑条目与最后物理偏移的对应关系需结合 engine 继续核对，暂不声称恢复安全性完整通过。
- request_hash 对对象递归排序、数组保留顺序，对 kind + NUL + canonical JSON 做 SHA-256，取前 16 字节编码成 32 位十六进制。
- agent_reservation_count 统计 kind 为 spawn_agent 的逻辑条目（含 pending/error），并非成功 agent 数。

## 已读测试范围

journal 测试覆盖普通持久化重放、pending 恢复完成、投影替换/只读、普通重复行拒绝、hash/kind 分歧、torn tail、无换行合法尾行、Unix symlink、超限读取与写入、非法完整行、稠密序号、持久化失败内存不推进、尾部 host error prune 的匹配与不匹配以及 hash 稳定性。本阶段仅阅读，未新运行 workflow 测试。

## engine/validate 阅读推进

已完整阅读 validate.rs 313 行、examples/validate.rs 23 行；engine.rs 已读 1–1230，生产实现 1–923 已全部覆盖，剩余 1231–1889 测试待读。包仍 pending。

- run_workflow 不自行 extract_meta；直接编译整个脚本，args 转为 Rhai scope。自然返回也 Completed；Dynamic 无法转 JSON 时降级 null。DEFAULT_MAX_OPS 为 100000000，但实际由 params.max_ops 决定。
- 引擎调用深度 64、表达式深度 128/64、字符串 16 MiB、数组/map 各 65536；DummyModuleResolver、禁 eval；timestamp、sleep 的整数/浮点重载及无参 exit 返回运行时错误。取消在 Rhai progress callback 检查，不直接中断 blocking_recv。
- 完成、暂停、等待用户、预算、取消、fatal 使用 ErrorTerminated ControlToken；嵌套函数/module 错误递归寻找 token，普通错误加 Rhai hint。host journal 错误 fatal。
- 结果 host_call 先占一个序号（最多 10000），计算 hash；完成结果直接重放，pending 复用 ID，无记录先写 UUIDv4 pending 再发送请求。同步 blocking_recv；channel/reply 丢失 fatal。Unsupported/Failed 保存 error sentinel 后返回可捕获错误；QuotaExceeded 返回可捕获错误而不 complete；Budget/Cancelled 终止并保留 pending。
- agent 单字符串重载直接创建 AgentOpts，没有 map 重载的 trim 非空检查。map 拒绝未知项，显式 prompt 参数覆盖 map.prompt；parallel 各项必须 map，最多 1024，先验证全部 options。
- 新 agent 先 reserve，一般 completed/pending 重放不新增 reserve。新 agent 的 Budget/Cancelled 释放额度，其他失败未走此释放分支。
- parallel 预查所有 hash 并一次预留新调用数量，逐项写 pending/发请求，随后按输入顺序 blocking_recv。完成结果持久化也按输入顺序，不随宿主完成到达顺序变化；修正上一阶段仅推测的普通并行结果乱序疑点。遇到提前 dispatch 错误部分分支 drain 已发回复，begin_operation 的 ? 分支不经过 drain。
- parallel 任何 Budget/Cancelled 使整批不写完成结果，释放 live_count，保留 pending（包括本批成功回复）；否则逐项写完成结果。终态优先于普通错误，多个终态保留遇到的第一个。drop reply 记 terminal sentinel，可恢复预算/取消时则不落完成记录。需要宿主复用 operation ID 才能解释外部效果去重，包本身不能证明 exactly-once。
- phase/log/print/debug/diagnostics_event 发无回复通知，不占 journal 序号，replayed 根据当前 seq 是否已有 journal 决定；发送失败忽略。diagnostics fields 必须可转 JSON 的 map。
- complete() 返回 null，complete(value) 转 JSON；pause 每次都终止、不记 journal。await_user 将原始 kind 字符串/message hash 后记录 null，首次 AwaitingUser，恢复相同检查点继续；alias 改写可能 hash 分歧。
- budget/render_template/write_scratch_file/read_scratch_file/git_diff_since 走相同结果 journal，但请求结构丢弃 operation_id；不能将持久 pending 等同于这些宿主操作的幂等性保证。fingerprint 用 request_hash，json_encode 对不可转换 Dynamic 同样转 null。
- validate 先 extract_meta，再用独立 stub 宿主线程、memory journal、10000000 ops dry-run；默认预算128，可调用显式预算入口。reserve 使用 saturating_add，release saturating_sub。agent 返回固定成功对象，文件/模板/diff 都返回 stub，不读写真实文件或调用 Git；预算 total/remaining 为 None。
- dry-run 接受 Completed/Paused/AwaitingUser，其他结果转 ValidationError::Run；摘要只对成功/暂停内容按 Unicode 字符截取 200。传自定义 args 时整体替代 default_probe_args。drop JoinHandle 不 join。
- validate example 从 stdin 读完整脚本，成功打印 META OK/RUN OK；meta 失败退出1、run 失败退出2，读取失败 expect panic。
- 已读 tests 覆盖 metadata失败、stub通过、暂停/等待有效、预算/并行限制、作者错误hint；engine前段测试覆盖 agent happy/unknown字段、catchable错误持久重放、await_user恢复、timestamp、args、pause、budget终止和纯循环取消。未新执行测试。

## 全包阅读完成

engine.rs 剩余1231–1889已全部阅读，至此全包8个Rust文件共3845行及Cargo.toml阅读完成。剩余工作是结构化能力映射和验证终态；在映射完成前继续保持pending。

剩余测试核对：1025项并行输入在发送前拒绝；10000结果host-call上限不可catch；部分并行setup在seq超限时drain已发reply；预算终态保留两个operation intent并在恢复再次请求两个宿主操作；取消/预算释放新reserve并由测试宿主在恢复前用journal.agent_reservation_count重建用量；普通并行失败和成功兄弟均持久化且重放不再次发送；并行error sentinel可catch；输出按输入顺序；预算查询重放返回原spent/reserved/remaining；journal写失败fatal；prompt编辑触发分歧；phase在已有result前replayed=true、末尾false；fingerprint稳定及JSON字符串转义。

这些测试使用模拟宿主，不能证明真实子Agent去重、session恢复、文件隔离或预算全局正确性。parallel顺序测试宿主按接收顺序立即回复，没有主动反转完成时序，源码的顺序等待是额外证据。多数测试drop宿主JoinHandle而非join，应区分主线程断言与宿主线程panic传播。

## workflow 测试终态

会话44678退出0；`cargo test --locked -p workflow`：65 passed、0 failed、0 ignored，doc-tests 0。日志 `/tmp/grow-workflow-inventory-tests.log`。默认当前macOS配置与模拟宿主测试通过，不扩大为真实session或跨平台验证。

## 能力映射完成

# workflow 逐包核查

包路径：`crates/codegen/workflow`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，已运行65项测试全部通过。

## 模块与开关

- `crates/codegen/workflow/Cargo.toml`
- `crates/codegen/workflow/examples/validate.rs`
- `crates/codegen/workflow/src/engine.rs`
- `crates/codegen/workflow/src/host.rs`
- `crates/codegen/workflow/src/journal.rs`
- `crates/codegen/workflow/src/lib.rs`
- `crates/codegen/workflow/src/meta.rs`
- `crates/codegen/workflow/src/run.rs`
- `crates/codegen/workflow/src/validate.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Workflow metadata extraction evaluation](../specs/workflow-execution/spec.md#requirement-workflow-metadata-extraction-evaluation)：元数据提取 SHALL 先检查首段 let meta/const meta 文本前缀，编译整个脚本，再以 args=UNIT、100000 operations 上限求值；忽略求值错误并读取 scope.meta map。
- [Workflow metadata shape and byte limits](../specs/workflow-execution/spec.md#requirement-workflow-metadata-shape-and-byte-limits)：WorkflowMeta/PhaseMeta SHALL 拒绝未知字段；name/description/title trim 后非空，name 为小写 ASCII 字母数字及单 hyphen、无首尾 hyphen；长度按 UTF-8 字节校验。
- [Workflow typed host protocol](../specs/workflow-execution/spec.md#requirement-workflow-typed-host-protocol)：工作流 SHALL 通过 typed unbounded channel 向宿主发送 reserve/release、spawn、phase/log/diagnostic、budget、template、scratch read/write 和 Git diff 请求；结果请求使用 oneshot。
- [Workflow engine limits and determinism guards](../specs/workflow-execution/spec.md#requirement-workflow-engine-limits-and-determinism-guards)：引擎 SHALL 应用调用方 max_ops、调用深度64、表达式深度128/64、字符串16MiB、数组和map各65536的限制，禁eval和模块解析，拒绝timestamp、sleep和无参exit。
- [Workflow termination and value conversion](../specs/workflow-execution/spec.md#requirement-workflow-termination-and-value-conversion)：控制函数 SHALL 通过不可捕获的终止token表达Completed/Paused/AwaitingUser/BudgetExceeded/Cancelled/Failed；普通错误保留可捕获运行时错误语义。
- [Workflow result call sequencing](../specs/workflow-execution/spec.md#requirement-workflow-result-call-sequencing)：结果调用 SHALL 分配从0递增的逻辑序号，上限10000；同序号的kind或请求hash不一致时fatal，已经完成的结果直接重放。
- [Workflow host failure persistence](../specs/workflow-execution/spec.md#requirement-workflow-host-failure-persistence)：宿主 Unsupported/Failed SHALL 写入host error sentinel后作为可捕获错误返回，重放同样抛错；Budget/Cancelled终止并保留pending。
- [Workflow agent invocation and reservations](../specs/workflow-execution/spec.md#requirement-workflow-agent-invocation-and-reservations)：新agent调用 SHALL 先向宿主reserve一次额度；已有completed或pending不新增reserve；新调用Budget/Cancelled释放本轮额度。
- [Workflow parallel dispatch and ordering](../specs/workflow-execution/spec.md#requirement-workflow-parallel-dispatch-and-ordering)：parallel SHALL 最多接受1024个options map，先校验全部选项与重放hash，再一次预留新项数量、逐项派发并按输入顺序收集结果。
- [Workflow parallel terminal recovery](../specs/workflow-execution/spec.md#requirement-workflow-parallel-terminal-recovery)：并行批次 SHALL 等待已收集的reply；任一Budget/Cancelled使本批保留pending、跳过全部live结果完成写入并释放本轮live_count。
- [Workflow notifications and replay flags](../specs/workflow-execution/spec.md#requirement-workflow-notifications-and-replay-flags)：phase/log/print/debug/diagnostics_event SHALL 发送无回复通知，以当前seq是否被journal覆盖标注replayed，不持久化独立结果条目。
- [Workflow pause and attention checkpoints](../specs/workflow-execution/spec.md#requirement-workflow-pause-and-attention-checkpoints)：pause SHALL 每次终止而不记journal；await_user SHALL 首次记录null并返回AwaitingUser，相同检查点恢复时继续执行。
- [Workflow utility host calls and pure helpers](../specs/workflow-execution/spec.md#requirement-workflow-utility-host-calls-and-pure-helpers)：budget/template/scratch read/write/Git diff SHALL 采用结果journal重放；fingerprint采用稳定请求hash，json_encode产生JSON文本。
- [Workflow injected journal storage](../specs/workflow-execution/spec.md#requirement-workflow-injected-journal-storage)：Journal SHALL 支持memory与注入JournalStorage，生产接口只使用read_bounded/append/truncate，不拥有环境文件路径。
- [Workflow journal line commit boundary](../specs/workflow-execution/spec.md#requirement-workflow-journal-line-commit-boundary)：journal恢复 SHALL 只接纳换行终止的JSON行，跳过完整空白行，截断任何未终止尾部；JournalEntry拒绝未知字段。
- [Workflow journal logical folding and projection](../specs/workflow-execution/spec.md#requirement-workflow-journal-logical-folding-and-projection)：恢复与只读project SHALL 要求逻辑条目从0稠密增长，最多10000项；带operation ID的同seq同kind/hash非pending物理行可替换逻辑结果。
- [Workflow journal intent completion and caps](../specs/workflow-execution/spec.md#requirement-workflow-journal-intent-completion-and-caps)：begin/complete/record SHALL 先追加物理行再推进内存；append包含newline并限制累计64MiB，memory也受限。
- [Workflow journal replay and reservation projection](../specs/workflow-execution/spec.md#requirement-workflow-journal-replay-and-reservation-projection)：replay SHALL 校验seq/kind/hash，缺项返回None；普通replay拒绝pending，replay_operation返回Pending或Completed；reservation count统计所有spawn_agent逻辑条目。
- [Workflow trailing host error pruning](../specs/workflow-execution/spec.md#requirement-workflow-trailing-host-error-pruning)：prune SHALL 只在最后逻辑结果的非空host error被failure_detail包含时截断最后物理行；普通结果pop，operation恢复pending并保留ID。
- [Workflow canonical request fingerprints](../specs/workflow-execution/spec.md#requirement-workflow-canonical-request-fingerprints)：请求hash SHALL 对JSON对象递归排序、保留数组顺序，对kind加NUL及canonical JSON进行SHA256，取前16字节为32位十六进制。
- [Workflow stub validation and authoring feedback](../specs/workflow-execution/spec.md#requirement-workflow-stub-validation-and-authoring-feedback)：validate SHALL 先extract_meta再用memory journal、10000000 ops及stub宿主执行；默认agent预算128，可显式传入预算。
- [Workflow validation example exit contract](../specs/workflow-execution/spec.md#requirement-workflow-validation-example-exit-contract)：validate示例 SHALL 从stdin读取脚本并打印META/RUN结果，metadata失败退出1，dry-run失败退出2。

## 边界

- 仍可提取已赋值的 meta；后续语法错误则在 compile 阶段失败。
- 读取求值后的 map，不宣称这是只读取首条 AST 的静态提取器。
- 返回对应长度或数量错误。
- 拒绝；去空白后相同但原始不同不由此集合判重，可选when_to_use/detail允许空串。
- 拒绝未知字段；opts支持prompt、label、model、agent_type、capability_mode、isolation_worktree、fork_context、resume_from、output_schema、phase；结果包含agent_id/success/output/cancelled/tokens_used/duration_ms。
- 本包只传递typed请求，真实权限、文件隔离及子Agent生命周期由宿主实现。
- 返回Cancelled；blocking_recv期间不由该progress回调主动中断。
- 编译脚本并注入args，不自动执行extract_meta；自然返回也形成Completed。
- 递归提取token并保留对应outcome。
- dynamic_to_value回退null；complete()返回null，json_encode同样使用该转换。
- 不可catch的Failed，超限结果请求不发送；通知不占该序号。
- 首次生成UUIDv4并先写intent再发送；恢复复用operation_id。
- fatal并保留已写intent。
- 返回可捕获错误而不complete；reserve阶段quota错误则BudgetExceeded。
- 拒绝未知项和trim后空prompt，显式prompt覆盖map.prompt。
- 本层直接构造AgentOpts而不执行map的非空检查，最终结果由宿主决定。
- 校验阶段返回错误，不发送spawn。
- 按输入顺序等待并构造结果数组；已有completed结果直接使用。
- 成功回复也未写completed，恢复使用原operation_id再次请求宿主；效果去重依赖宿主。
- 记录各live结果和error sentinel，重放不重启已完成兄弟；terminal marker优先于普通错误，多个marker保留先遇到者。
- replayed为false，即使整个脚本正在恢复。
- 忽略发送错误；diagnostics fields转换错误仍返回运行时错误。
- 解析为对应PauseKind，非法kind报错；serde使用snake_case枚举值。
- 请求hash改变导致分歧，别名语义相同不免除hash检查。
- 请求结构不传operation_id，不能由本包保证外部副作用只执行一次。
- JSON编码转义对应字符；预算重放返回记录时数值而非重新查询。
- 分别视为空journal或UnsafeRestore；其他IO错误传播。
- 依赖storage履行限制，不对返回Vec再次限长；cfg(test)文件实现的O_NOFOLLOW/sync_data不代表生产宿主保证。
- 不作为提交条目，truncate到尾部开始。
- Parse并携带行号，不按torn tail丢弃。
- Sequence错误；operation折叠保留ID，当前实现仍可接纳第二次非pending替换，不能宣称所有重复完成被拒绝。
- 不写storage、不更新bytes/last_line_start，不用于推导随后prune或写入安全性。
- 追加完成物理行并替换逻辑结果；直接complete第二次拒绝。
- 内存不推进；storage部分写入语义仍由宿主负责。record/begin的序号检查自身未施加10000条上限。
- 也计入这些条目，不仅统计成功结果。
- 返回原operation_id，不生成新ID。
- 不prune。
- last_line_start已清空，可返回无法定位偏移错误，不保证连续多次prune。
- hash一致；kind或数组顺序改变参与hash。
- 成功报告name/phase数量及最多200字符内容摘要；Failed/Budget/Cancelled返回Run错误。
- 返回固定stub数据，不执行真实Agent、文件读写或Git；自定义args整体替代默认探针参数。
- expect panic；成功打印META OK与RUN OK。
- 相关meta/runtime校验错误可附带拆分+=、重命名或检查JSON/string类型提示。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
