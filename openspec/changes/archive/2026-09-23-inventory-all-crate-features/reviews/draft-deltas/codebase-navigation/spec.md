## ADDED Requirements

### Requirement: Graph language registry
语言注册表 SHALL 提供Rust、TypeScript、JavaScript、Go、Python五种配置，精确匹配rs/ts/tsx/js/jsx/go/py扩展名。

#### Scenario: Graph language registry boundary
- **WHEN** 输入mjs、pyi或大写扩展名
- **THEN** 不提供对应配置；TS与TSX共用TSX grammar，TS和JS仍是不同family。

证据：`crates/codegen/codebase-graph/src/languages/mod.rs` — `LanguageRegistry`。

### Requirement: Graph query version identity
query版本 SHALL 由排序后的主语言ID及query文本生成，供缓存判断重建。

#### Scenario: Graph query version identity boundary
- **WHEN** 只有grammar库版本、namespace或extension配置改变
- **THEN** 不保证hash随之改变；DefaultHasher不构成跨Rust版本稳定格式。

证据：`crates/codegen/codebase-graph/src/languages/mod.rs` — `compute_query_hash`。

### Requirement: Rust graph query coverage
Rust query SHALL 提取所列ADT、trait、module、macro、function、const、static及选定调用和类型引用。

#### Scenario: Rust graph query coverage boundary
- **WHEN** 局部变量、import或语义关系未被query覆盖
- **THEN** 不得据此宣称完整Rust符号解析；重叠capture保留重复可能。

证据：`crates/codegen/codebase-graph/src/languages/rust.rs` — `function`。

### Requirement: TypeScript graph query coverage
TypeScript query SHALL 包含函数签名、类、interface、类型标识符、选定解构和参数、JSX及export引用。

#### Scenario: TypeScript graph query coverage boundary
- **WHEN** 变量初始化为任意call表达式
- **THEN** 可被标为function；member调用主要引用object而非property，不提供类型推断。

证据：`crates/codegen/codebase-graph/src/languages/ts.rs` — `function`。

### Requirement: JavaScript graph query coverage
JavaScript query SHALL 提取其函数、类、变量及调用模式，独立于TypeScript查询。

#### Scenario: JavaScript graph query coverage boundary
- **WHEN** 存在member调用或import alias
- **THEN** 按JS capture记录property或原始名字，不保证与TS查询覆盖一致。

证据：`crates/codegen/codebase-graph/src/languages/javascript.rs` — `function`。

### Requirement: Python and Go graph queries
Python SHALL 提取class/function定义及call引用；Go提供函数方法类型常量变量及调用类型和import相关capture。

#### Scenario: Python and Go graph queries boundary
- **WHEN** Go使用短变量声明或import literal
- **THEN** 不能假定短声明已覆盖；literal名字可保留引号，Python查询不是完整变量引用索引。

证据：`crates/codegen/codebase-graph/src/languages/python.rs` — `function`；`crates/codegen/codebase-graph/src/languages/golang.rs` — `function`。

### Requirement: Graph coordinate and range arithmetic
Position SHALL 保存byte列和offset并提供零基与一基转换；Range包含判断含end，len采用end减start。

#### Scenario: Graph coordinate and range arithmetic boundary
- **WHEN** 调用byte_size或构造反向范围
- **THEN** byte_size采用差值加一；不验证反向范围或溢出，不应当作LSP UTF16坐标。

证据：`crates/codegen/codebase-graph/src/types/range.rs` — `byte_size`。

### Requirement: Graph file metadata freshness
FileMeta SHALL 以size及mtime秒纳秒比较新鲜度，不计算内容hash。

#### Scenario: Graph file metadata freshness boundary
- **WHEN** stat失败或mtime无效
- **THEN** stale判定失败侧视为过时；mtime可回落零，同size同mtime内容变化不保证检测。

证据：`crates/codegen/codebase-graph/src/scope_graph/graph.rs` — `is_file_stale`。

### Requirement: Graph byte string interning
StringInterner SHALL 以byte arena及hash桶内容比较复用ID，提供byte与UTF8访问。

#### Scenario: Graph byte string interning boundary
- **WHEN** 超出u32 offset或u16长度、跨interner使用ID
- **THEN** 不提供checked转换、归属或generation保证；clear可复用ID，compact不清理无引用字符串。

证据：`crates/codegen/codebase-graph/src/interner.rs` — `StringInterner`。

### Requirement: Graph public event and location models
包 SHALL 同时保留actor的kind加paths事件与types中的分支枚举事件，以及导航和序列化位置模型。

#### Scenario: Graph public event and location models boundary
- **WHEN** 构造actor事件为空或多于两个path
- **THEN** 类型本身不验证arity；types rename主路径为to，位置模型不校验绝对路径。

证据：`crates/codegen/codebase-graph/src/index_manager.rs` — `FileEvent`；`crates/codegen/codebase-graph/src/types/file_event.rs` — `Renamed`；`crates/codegen/codebase-graph/src/types/location.rs` — `Location`。

### Requirement: Local scope graph APIs
ScopeGraph SHALL 提供local/global/hoisted定义、scope/import和引用插入及范围查找。

#### Scenario: Local scope graph APIs boundary
- **WHEN** 调用insert_ref
- **THEN** 按名字与namespace遍历祖先候选，不在shadowing处停止；没有候选不插入，unconditional可插入孤立引用。

证据：`crates/codegen/codebase-graph/src/scope_graph/graph.rs` — `insert_ref`。

### Requirement: Query graph construction boundary
query构图 SHALL 将匹配定义放到root并记录孤立引用，不从现有query构造完整scope/import或RefToDef解析边。

#### Scenario: Query graph construction boundary boundary
- **WHEN** 调用references-with-definitions
- **THEN** 不能将所有捕获引用视为已解析；from_symbols丢弃名字且root范围来自首个occurrence。

证据：`crates/codegen/codebase-graph/src/scope_graph/graph.rs` — `scope_graph_from_definitions_query`。

### Requirement: Fast graph capture extraction
fast提取 SHALL 按capture前缀分类定义引用和alias，不自动去重。

#### Scenario: Fast graph capture extraction boundary
- **WHEN** 名字包含非法UTF8
- **THEN** bulk采用lossy文本；actor直接intern路径跳过非法UTF8 capture，两个入口边界不同。

证据：`crates/codegen/codebase-graph/src/scope_graph/graph.rs` — `extract_symbols_fast`。

### Requirement: Graph occurrence and file accounting
索引 SHALL 分别存储occurrence、graph、file metadata；file_count与indexed_files由metadata决定。

#### Scenario: Graph occurrence and file accounting boundary
- **WHEN** 只add_definition或重复add_file
- **THEN** 不自动建立metadata；重复添加可追加旧occurrence，is_indexed检查graph而非metadata。

证据：`crates/codegen/codebase-graph/src/scope_graph/graph.rs` — `add_file`。

### Requirement: Graph file removal and rename
remove_file SHALL 删除该路径的graph、metadata及反向occurrence，但保留interner和全局alias。

#### Scenario: Graph file removal and rename boundary
- **WHEN** rename目标已有索引
- **THEN** 当前实现可覆盖目标反向记录而保留旧occurrence，不保证替换事务或alias清理。

证据：`crates/codegen/codebase-graph/src/scope_graph/graph.rs` — `rename_file`。

### Requirement: Graph alias resolution
alias SHALL 在全局名字空间记录单跳原名及反向关系，定义查原名，引用纳入原名和直接alias。

#### Scenario: Graph alias resolution boundary
- **WHEN** alias重指向、文件删除或alias链
- **THEN** 不保证清除旧反向关系或递归解析，不按文件及语言隔离。

证据：`crates/codegen/codebase-graph/src/scope_graph/graph.rs` — `add_alias`。

### Requirement: Graph name lookup and ranking
smart定义查询 SHALL 以path与line去重，再按语言family及path排序；引用查询保留重复。

#### Scenario: Graph name lookup and ranking boundary
- **WHEN** 无上下文或同名来自多个scope
- **THEN** 不进行语义消歧；top引用数平局及原始hash遍历顺序不稳定。

证据：`crates/codegen/codebase-graph/src/scope_graph/graph.rs` — `find_definitions_smart`。

### Requirement: Graph compact binary persistence
索引 SHALL 使用SGIX v1 little-endian格式保存interner、occurrence、metadata、alias及query版本，不序列化graph。

#### Scenario: Graph compact binary persistence boundary
- **WHEN** 保存中断或并发读取
- **THEN** 直接create与flush无原子rename/fsync保证；compact只缩减容量，不是垃圾回收。

证据：`crates/codegen/codebase-graph/src/scope_graph/graph.rs` — `write_to`。

### Requirement: Graph cache input validation
load SHALL 对不足四字节返回IO错误，其他非magic返回None，读取计数驱动结构恢复。

#### Scenario: Graph cache input validation boundary
- **WHEN** 恶意长度、错误ID、尾部数据或query tag缺失
- **THEN** 不保证资源限额、ID关系及尾部验证；query tag读取错误或未知值回Legacy，已识别tag后截断仍可报错。

证据：`crates/codegen/codebase-graph/src/scope_graph/graph.rs` — `read_from`。

### Requirement: Graph cache wrapper lifecycle
cache包装 SHALL 提供默认路径、存在与大小探测、同步load/save及后台保存。

#### Scenario: Graph cache wrapper lifecycle boundary
- **WHEN** 调用后台保存
- **THEN** 线程持有index且不提供join终态，错误警告；包装本身不保证锁或原子发布。

证据：`crates/codegen/codebase-graph/src/manager/cache.rs` — `save_index`。

### Requirement: Graph workspace lock semantics
try_lock SHALL 在同进程协调共享Load及互斥操作，独占操作另检查并写磁盘锁。

#### Scenario: Graph workspace lock semantics boundary
- **WHEN** 两个进程同时检查空锁文件
- **THEN** 检查与写入不原子，不构成flock或create_new互斥；Load忽略磁盘锁，释放时无ownertoken校验。

证据：`crates/codegen/codebase-graph/src/manager/lock.rs` — `try_lock`。

### Requirement: Graph stale lock policy
磁盘锁 SHALL 按请求操作timeout与pid存活判断是否可替换。

#### Scenario: Graph stale lock policy boundary
- **WHEN** 请求Load或Save、Build、Refresh
- **THEN** 阈值分别120、600、300秒，非原持锁操作阈值；超时可替换存活进程锁，Unix kill0权限错误也判dead。

证据：`crates/codegen/codebase-graph/src/manager/lock.rs` — `try_acquire_file_lock`。

### Requirement: Graph build enumeration
IndexBuilder SHALL 优先采用git2非空受支持tracked列表，否则回退walk。

#### Scenario: Graph build enumeration boundary
- **WHEN** 仓库有受支持tracked及untracked文件
- **THEN** 快速路径遗漏untracked，hidden/gitignore选项不作用于该路径；walk错误跳过。

证据：`crates/codegen/codebase-graph/src/manager/builder.rs` — `IndexBuilder`。

### Requirement: Graph build resource controls
builder SHALL 默认N减一且至少一线程、chunk100、batch5000，batch有效大小不小于chunk。

#### Scenario: Graph build resource controls boundary
- **WHEN** chunk设为0或请求自定义registry
- **THEN** chunk0可panic；收集及hash用配置registry，但解析使用新默认registry，不保证自定义解析一致。

证据：`crates/codegen/codebase-graph/src/manager/builder.rs` — `with_build_batch_size`。

### Requirement: Graph parsing eligibility
解析 SHALL 预检非空且不超过5MiB，首8000bytes NUL判binary，然后全文件读取与tree-sitter解析。

#### Scenario: Graph parsing eligibility boundary
- **WHEN** 文件在stat后增长或有语法error node
- **THEN** 不重新执行大小上限或拒绝error tree；无parse deadline，query失败可替换空query，metadata来自读取前。

证据：`crates/codegen/codebase-graph/src/manager/builder.rs` — `process_file_fast`。

### Requirement: Graph disk position navigation
Navigator SHALL 从当前磁盘文件与byte行列查标识符，再查询缓存名字索引；名字查询无需读文件。

#### Scenario: Graph disk position navigation boundary
- **WHEN** 当前磁盘内容与索引不同步
- **THEN** 不保证同一快照语义；位置读不受5MiB限制，include_definition前插定义且仅按path/line避重。

证据：`crates/codegen/codebase-graph/src/navigation.rs` — `Navigator`。

### Requirement: Graph actor identity and channels
IndexManager SHALL 以canonical root的Weak Arc表复用handle，使用无界队列串行处理命令。

#### Scenario: Graph actor identity and channels boundary
- **WHEN** 相同root携带不同配置或外部clone内部handle
- **THEN** 复用忽略新配置；弱身份不覆盖内部Sender clone，不能保证永远仅一个actor；shutdown只发送不join。

证据：`crates/codegen/codebase-graph/src/index_manager.rs` — `spawn`。

### Requirement: Graph actor query responses and snapshots
actor SHALL 提供blocking及async查询、轻量统计和Arc快照，更新通过COW隔离现存快照。

#### Scenario: Graph actor query responses and snapshots boundary
- **WHEN** 响应通道意外关闭
- **THEN** 部分API expect会panic，轻量统计折叠为None；没有响应timeout或有界排队保证。

证据：`crates/codegen/codebase-graph/src/index_manager.rs` — `get_snapshot`。

### Requirement: Graph actor initialization
actor SHALL 在启动时加载cache并比较query版本，失败或不匹配则fresh build；构建失败回退空Legacy索引。

#### Scenario: Graph actor initialization boundary
- **WHEN** cache加载成功
- **THEN** 可在一次后台验证完成前服务查询；初始化load/build/save未统一受workspace lock保护。

证据：`crates/codegen/codebase-graph/src/index_manager.rs` — `build_fresh_index`。

### Requirement: Graph incremental event coalescing
actor SHALL 合并连续文件事件，遇到其他命令先flush；rename至少两path拆Removed和Created。

#### Scenario: Graph incremental event coalescing boundary
- **WHEN** 已有文件同批Modified后Removed
- **THEN** 当前coalescer取消待办，可保留旧索引；多余rename path忽略，跨path HashMap无稳定顺序。

证据：`crates/codegen/codebase-graph/src/index_manager.rs` — `CoalescedEvents`。

### Requirement: Graph incremental replacement
增量 SHALL 按language与全部路径组件hidden过滤，先删旧项再读取解析。

#### Scenario: Graph incremental replacement boundary
- **WHEN** 读取失败、文件变大、或删除事件路径hidden
- **THEN** 旧项可消失，或过滤阻止删除而残留；不验证路径在root内，也不完整执行gitignore。

证据：`crates/codegen/codebase-graph/src/index_manager.rs` — `reindex_file`。

### Requirement: Graph cache save scheduling
actor SHALL 在事件处理时检查60秒保存间隔，另在重建、后台刷新、shutdown路径保存。

#### Scenario: Graph cache save scheduling boundary
- **WHEN** 没有新事件
- **THEN** 不因时间经过自动保存；更新计数累计而非每次保存重置，保存本身无锁事务保证。

证据：`crates/codegen/codebase-graph/src/index_manager.rs` — `apply_coalesced`。

### Requirement: Graph background validation limits
cache命中后 SHALL 启动一次后台扫描，持Refresh锁扫描后发命令，Busy时跳过无retry。

#### Scenario: Graph background validation limits boundary
- **WHEN** 缓存相对路径但cwd不是root
- **THEN** 当前stat未join root且walk为绝对路径，可能误判deleted或new；锁不覆盖actor应用，没有快照版本防止过时结果。

证据：`crates/codegen/codebase-graph/src/index_manager.rs` — `process_background_refresh`。

### Requirement: Graph CLI command and cache policy
code-graph SHALL 提供index/definition/references/stats及自定义cache，index每次重建，force参数不影响行为。

#### Scenario: Graph CLI command and cache policy boundary
- **WHEN** 其他命令成功读cache
- **THEN** 直接使用而不执行actor freshness/query检查；参数合法性检查发生在load/build之后，完整位置优先symbol。

证据：`crates/codegen/codebase-graph/src/bin/code_graph.rs` — `load_or_build_index`。

### Requirement: Graph CLI output format
CLI SHALL 输出symbol及path/line位置，references可包含alias名，stats列top10引用。

#### Scenario: Graph CLI output format boundary
- **WHEN** 设置--json
- **THEN** 结果段为JSON但stdout前有cache状态文本，不能声称整条stdout是单个JSON；错误也println且失败exit1。

证据：`crates/codegen/codebase-graph/src/bin/code_graph.rs` — `print_json`。

### Requirement: Graph benchmark entrypoints
bench_index SHALL 打印query编译状态及默认builder统计，bench_file_listing比较CLI/git2/index-only计时。

#### Scenario: Graph benchmark entrypoints boundary
- **WHEN** query失败或列表数量不同
- **THEN** 基准仍可继续；非NUL CLI路径处理不可靠且数据集未校验等价，不能当严格性能证明。

证据：`crates/codegen/codebase-graph/src/bin/bench_index.rs` — `main`；`crates/codegen/codebase-graph/src/bin/bench_file_listing.rs` — `main`。
