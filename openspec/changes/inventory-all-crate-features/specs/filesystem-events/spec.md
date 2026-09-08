## ADDED Requirements

### Requirement: Filesystem semantic event protocol
FsEvent SHALL 使用type/data snake_case表达文件组、Git元数据、操作开始和完成；文件组共享Created/Modified/Removed/Renamed一种kind。

#### Scenario: 行为边界
- **WHEN** 需要逐路径独立kind或commit详情
- **THEN** 本层不提供；Git丰富信息与多root组合属于workspace层，枚举non_exhaustive。

证据：`crates/codegen/fsnotify/src/event.rs` — `FsEvent`。

### Requirement: Filesystem source configuration and startup
FsConfig SHALL 默认100ms debounce和空ignore；start要求当前Tokio runtime，start_on使用传入handle启动异步循环。

#### Scenario: 行为边界
- **WHEN** runtime不存在或child watch部分失败
- **THEN** start返回NoRuntime；root/debouncer初始化失败WatcherStart，部分child失败可能仍ready，不保证成功即全覆盖。

证据：`crates/codegen/fsnotify/src/source.rs` — `FsConfig`。

### Requirement: Filesystem canonical shared source registry
shared SHALL 以canonical路径Weak缓存存活source，首次配置生效；并发创建在registry锁外完成再选择已有赢家。

#### Scenario: 行为边界
- **WHEN** 同目录请求不同配置或最后Arc释放
- **THEN** 存活source忽略新配置；最后Arc释放销毁，后续重新创建；canonical失败使用原路径key。

证据：`crates/codegen/fsnotify/src/source.rs` — `shared`。

### Requirement: Filesystem shared runtime and statistics
共享循环 SHALL 优先首次注册的runtime handle，否则当前runtime；stats剔除无strong引用条目并返回创建/复用计数。

#### Scenario: 行为边界
- **WHEN** 竞争创建败者或source已shutdown仍持Arc
- **THEN** 败者计reuse但曾实际创建watcher；live按Arc而非健康状态统计，handle注册不证明runtime仍运行。

证据：`crates/codegen/fsnotify/src/source.rs` — `stats`。

### Requirement: Filesystem subscription and shutdown lifetime
订阅 SHALL 使用容量256 broadcast独立backlog；source.shutdown只cancel异步循环，Drop再随handle销毁OS线程。

#### Scenario: 行为边界
- **WHEN** 订阅者落后或调用shutdown但保留source
- **THEN** 落后可Lagged且无重放；OS watcher仍由handle持有，最终Drop发送Shutdown并join，没有硬退出期限。

证据：`crates/codegen/fsnotify/src/source.rs` — `shutdown`；`crates/codegen/fsnotify/src/watcher.rs` — `impl Drop for FsNotifyHandle`。

### Requirement: Filesystem VCS discovery and internal paths
source SHALL 初始化时发现Git与Sapling目录，以component前缀剔除内部路径；Sapling关闭仍排除发现的内部目录。

#### Scenario: 行为边界
- **WHEN** 运行后新建VCS目录或Git metadata分类
- **THEN** 不动态重发现；只精确HEAD/index/packed-refs/FETCH_HEAD及refs前缀分类，非UTF8不能分类。

证据：`crates/codegen/fsnotify/src/source.rs` — `discover_vcs`；`crates/codegen/fsnotify/src/paths.rs` — `classify_git_path`。

### Requirement: Filesystem VCS lock and parent sampling
锁采样 SHALL 检查Git直属index.lock/gc.pid和启用时Sapling wlock；HEAD token连接原始Git HEAD文本与dirstate前20字节hex。

#### Scenario: 行为边界
- **WHEN** 符号ref对应commit变化或读取失败
- **THEN** 不解析ref目标commit；失败贡献空segment且可能改变token。Sapling先lstat后open不保证竞争下文件身份，I/O同步执行。

证据：`crates/codegen/fsnotify/src/source.rs` — `read_head`。

### Requirement: Filesystem merged operation state machine
锁操作 SHALL 释放后等待500ms settle，期间重新加锁保留最初HEAD和起始时间，不重复Started；到期无锁才Completed。

#### Scenario: 行为边界
- **WHEN** HEAD变化或cooldown中重新加锁
- **THEN** 变化进入500ms cooldown，不变Idle；cooldown重新加锁开始新操作。

证据：`crates/codegen/fsnotify/src/state.rs` — `drive`。

### Requirement: Filesystem stale lock warning semantics
StaleWarn SHALL 对Locked或Settling跨合并操作严格超过60秒仅报警一次，离开两状态后重置。

#### Scenario: 行为边界
- **WHEN** 锁长期静默或释放事件丢失
- **THEN** source只在raw事件后检查，无Locked周期poll；不强制解锁，不保证60秒自动报警或恢复。

证据：`crates/codegen/fsnotify/src/state.rs` — `StaleWarn`。

### Requirement: Filesystem event loop operation ordering
事件循环 SHALL biased优先cancel再raw再timer，用last_idle_head作为操作基线；批中锁路径即使已消失仍合成Started并settle。

#### Scenario: 行为边界
- **WHEN** raw连续就绪或下轮锁仍在debounce
- **THEN** timer可能延迟，基线存在已记录竞争可导致head_changed=false；无事件完整性保证。

证据：`crates/codegen/fsnotify/src/source.rs` — `event_loop`。

### Requirement: Filesystem operation event suppression
process_event SHALL 先发状态transition，Idle发送排序去重GitMeta；Locked/Settling发送文件事件供消费者缓存。

#### Scenario: 行为边界
- **WHEN** 处于Cooldown
- **THEN** GitMeta与FilesChanged均被丢弃，无本层补发；路径组不在此去重，send错误忽略。

证据：`crates/codegen/fsnotify/src/source.rs` — `process_event`。

### Requirement: Filesystem raw event merge policy
raw合并 SHALL 丢Access/Any/Other，非rename按path合并且Removed胜出、Created优先Modified；rename保留paths并后置。

#### Scenario: 行为边界
- **WHEN** Remove后Create或跨kind事件
- **THEN** 仍Removed；HashMap组及组内顺序不稳定，rename可与普通组重复路径，不保留全部OS因果顺序。

证据：`crates/codegen/fsnotify/src/watcher.rs` — `merge_events`。

### Requirement: Filesystem custom glob precedence
custom glob SHALL 将无绝对或**/前缀的模式补**/，首!作为include，include优先ignore，非法模式warning跳过。

#### Scenario: 行为边界
- **WHEN** include与gitignore同时匹配
- **THEN** callback include先放行，可覆盖gitignore；watch选择已被ignore walker剪掉的目录不能借include重新进入。

证据：`crates/codegen/fsnotify/src/watcher.rs` — `build_globsets`。

### Requirement: Filesystem ignore cache scope
事件GitignoreCache SHALL 从path父目录向上读取.gitignore，以mtime缓存，深层ignore或whitelist立即决定。

#### Scenario: 行为边界
- **WHEN** 同mtime改内容或删除目录
- **THEN** 同mtime不重载；is_dir按处理时磁盘值，删除目录可能按文件评估；本cache不读取global/info-exclude/.ignore。

证据：`crates/codegen/fsnotify/src/watcher.rs` — `GitignoreCache`。

### Requirement: Filesystem watcher VCS whitelist
watcher SHALL 以正斜杠lossy路径contains允许Git index/HEAD/FETCH_HEAD/refs/packed-refs/gc.pid及启用时.sl/wlock。

#### Scenario: 行为边界
- **WHEN** 名称带后缀或Windows分隔符
- **THEN** 此层不是精确component分类，不能推导全平台严格路径等价；最终source另按发现目录分类排除。

证据：`crates/codegen/fsnotify/src/watcher.rs` — `is_git_path_for_watcher`。

### Requirement: Filesystem watch strategy and budget selection
策略 SHALL 精确env1/true选PerDir、0/false选Fanout，否则Linux PerDir其余Fanout；Sapling仅精确0/false关闭。

#### Scenario: 行为边界
- **WHEN** 预算env缺失非法或0
- **THEN** 默认49152，parse正usize有效；不读取系统剩余额度，root和VCS额外watch不含在该子目录预算。

证据：`crates/codegen/fsnotify/src/watcher.rs` — `max_watch_budget`。

### Requirement: Filesystem ignore aware directory selection
目录选择 SHALL 开启.gitignore/global/exclude/.ignore、允许hidden且不follow symlink，PerDir剪VCS/custom子树后全量收集浅层排序。

#### Scenario: 行为边界
- **WHEN** 树很大或显式root被祖先ignore
- **THEN** 预算不限制选择遍历内存时间；root选择和事件过滤有不同ignore边界，不能推导完整事件放行。

证据：`crates/codegen/fsnotify/src/watcher.rs` — `select_per_dir_watch_dirs`。

### Requirement: Filesystem fanout and recursive fallback
Fanout SHALL 在非忽略一级目录<=64时非递归root加递归child，超过64用递归root。

#### Scenario: 行为边界
- **WHEN** 后来一级目录变多或ignore编辑
- **THEN** reconcile只结构事件触发且不再执行64cap切换；新增树无文件backfill，recursive fallback可覆盖watch层排除本可省下的树。

证据：`crates/codegen/fsnotify/src/watcher.rs` — `reconcile_top_level_watches`。

### Requirement: Filesystem surgical metadata watches
PerDir Git SHALL 非递归gitdir和存在refs，递归存在heads/tags；Sapling非递归.sl。真实.git目录直接采用，gitlink/symlink经git2验证。

#### Scenario: 行为边界
- **WHEN** worktree缺refs或后来新建heads目录
- **THEN** 不补watch共同gitdir全部refs，不专门动态arm新增VCS命名空间；VCS失败warning仍可ready。

证据：`crates/codegen/fsnotify/src/watcher.rs` — `per_dir_git_watches`；`crates/codegen/fsnotify/src/watcher.rs` — `find_git_dir`。

### Requirement: Filesystem staged watch arming
PerDir SHALL 同步arm一级目录和VCS，深层pending<=4096同步清空，否则ready后每块512后台arm。

#### Scenario: 行为边界
- **WHEN** pending失败或初始化期间深层文件变化
- **THEN** 失败弹出不自动重试，ready不证明后台完成，可能丢启动事件；持续command可推迟pending。

证据：`crates/codegen/fsnotify/src/watcher.rs` — `arm_pending_chunk`。

### Requirement: Filesystem dynamic subtree maintenance
PerDir更新 SHALL 按当前lstat判add/prune，结构事件仍存在目录先prune再add，非目录候选prune。

#### Scenario: 行为边界
- **WHEN** pending目录被剪或动态add占用预算
- **THEN** command不清理pending，后台arm不重验预算/ignore；预算非严格总上限。

证据：`crates/codegen/fsnotify/src/watcher.rs` — `scan_per_dir_updates`。

### Requirement: Filesystem subtree creation backfill
新增子树 SHALL 惰性watch目录并将walk发现的非目录项分批512作为Created回填，跳过已watch根与symlink根。

#### Scenario: 行为边界
- **WHEN** watch失败或触预算
- **THEN** 失败仍可能继续walk，达到预算break剩余遍历；可重复或漏事件，无界通道，不能无条件承诺never lost。

证据：`crates/codegen/fsnotify/src/watcher.rs` — `add_subtree_watches`。

### Requirement: Filesystem watcher timeout and callback failures
初始化 SHALL 等待ready最多配置timeout默认10秒，超时或ready断开返回Timeout并排队Shutdown。

#### Scenario: 行为边界
- **WHEN** 慢同步初始化尚未结束或debouncer报错
- **THEN** timeout不立即中断线程，后续处理Shutdown；callback错误只warning，无自动补扫信号。

证据：`crates/codegen/fsnotify/src/watcher.rs` — `start_with_timeout`。

### Requirement: Filesystem measurement utilities
startup benchmark SHALL 区分12000目录正式bench与1200目录smoke，仅测start到ready；watch_stats提供生成树和多轮计数测量。

#### Scenario: 行为边界
- **WHEN** 在非Linux运行或iters为0
- **THEN** kernel计数为0不代表无watch，250ms一次不变仅估算armed；iters0索引失败，gen会覆写指定目录模拟文件，非无损项目生成器。

证据：`crates/codegen/fsnotify/benches/startup.rs` — `bench_startup`；`crates/codegen/fsnotify/examples/watch_stats.rs` — `inotify_watches`。

### Requirement: Nix Linux inotify interface
Inotify SHALL create an instance, add/remove watches, read variable-length event buffers through InotifyEvent/WatchDescriptor, expose raw descriptor conversion, and map libc failures to Errno.

#### Scenario: Nix Linux inotify interface implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/inotify.rs` — `struct AddWatchFlags`；`third_party/nix-ohos/src/sys/inotify.rs` — `struct InitFlags`；`third_party/nix-ohos/src/sys/inotify.rs` — `struct Inotify`；`third_party/nix-ohos/src/sys/inotify.rs` — `struct WatchDescriptor`；`third_party/nix-ohos/src/sys/inotify.rs` — `struct InotifyEvent`；`third_party/nix-ohos/src/sys/inotify.rs` — `fn init`；`third_party/nix-ohos/src/sys/inotify.rs` — `fn add_watch`；`third_party/nix-ohos/src/sys/inotify.rs` — `fn rm_watch`；`third_party/nix-ohos/src/sys/inotify.rs` — `fn read_events`；`third_party/nix-ohos/src/sys/inotify.rs` — `const BUFSIZ`；`third_party/nix-ohos/src/sys/inotify.rs` — `fn as_raw_fd`；`third_party/nix-ohos/src/sys/inotify.rs` — `fn from_raw_fd`。
