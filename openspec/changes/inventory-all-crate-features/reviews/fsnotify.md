# fsnotify 逐包审阅（进行中）

完整读取Cargo.toml、lib/event/error/paths/state；source读取至250行，watcher、source其余、集成测试、bench/example待完成。包保持pending。

- 包无Cargo feature声明，notify8/debouncer-full0.5、ignore/globset/git2及tokio支撑本地事件源。7份src共5951行；另integration370、bench169、example211行也属于盘点范围，总6701行。
- 公开FsEventSource/FsConfig/shared/runtime/stats、事件与错误、SETTLE_MS。单root，workspace层负责多root组成和Git丰富信息，不能从本包事件直接承诺commit/branch内容。
- FsEvent serde type/data snake_case，非穷尽enum，包含FilesChanged(paths,同一个kind)、GitMetaChanged、GitOperationStarted/Completed(head_changed)。FsEventKind Created/Modified默认/Removed/Renamed；GitMetaKind Head/Index/Refs/FetchHead。分组不携带逐path独立kind。FsNotifyError非序列化，WatcherStart boxedsource/Timeout/NoRuntime。
- classify_git_path先component strip_prefix(git_dir)，再UTF8相对路径精确HEAD/FETCH_HEAD/index/packed-refs或refs斜杠前缀；非UTF8/其他git内部/锁返回None。测试覆盖worktree gitdir与.git-backup不误判；这里只分类，实际监听覆盖待watcher核对。
- 全局runtime handle首次设置生效，shared优先该handle否则当前runtime；start仅当前runtime。注册handle不证明runtime仍活着。registry Weak按dunce canonical路径共享，canonical失败原样key，配置只创建时采用，存活同key忽略后续不同config。
- shared先锁查找，锁外耗时创建，再锁检查竞争结果；竞争败者新source丢弃，返回先登记对象。created_total仅登记赢家计数，不能按注释当作所有实际创建OS watcher数；reused_total包含竞争败者（并非全部避免创建）。stats剔除strong_count0，计数Relaxed独立读取非原子整体快照，live表示Arc存活而非底层watcher健康。
- FsConfig默认debounce100ms/空ignore，builder直接赋值，无数值范围校验。start_on先canonical cwd，与watcher共用一次sapling_enabled结果；启动raw watcher后discover_vcs、创建容量256 broadcast。余下初始化、订阅/退出及丢事件行为待读。
- 锁状态机纯输入驱动：Idle/Cooldown遇锁=>Locked并Started；Locked释放=>Settling直到now+500ms；Settling遇锁（即便deadline已过但尚未处理）恢复同一Locked，保留最初HEAD与since，不重复Started。Settling无锁且now>=until才Completed，以Option<String>精确比较HEAD，变更进入给定cooldown（source常量500ms），未变更直接Idle；Cooldown到期无锁发内部CooldownEnded，有锁优先Started。
- stale warning仅Locked或Settling持续严格>60秒时一次返回elapsed，跨短释放间隙保持latch；离开这两状态才重置，不自动解锁或结束操作。状态机使用调用方Instant/HEAD/lock事实，不访问磁盘；实际HEAD如何读取以及事件驱动定时器待读source。
- state测试13项覆盖settle/relock跨burst比较、到期边界、cooldown重新加锁、stale合并期；仅纯状态机，不代替真实OS监听覆盖。

## Source事件循环（推进至860行）

生产source逻辑全部读完，测试仅前段，861–1498仍待完成。

- start_on在raw watcher初始化后spawn detached event_loop，无保存JoinHandle或启动完成握手；subscribe独立broadcast backlog，容量256、慢消费者可Lagged，无重放/补偿API。shutdown只cancel事件循环token；FsNotifyHandle仍在source字段，OS watcher停止是否另有机制待watcher读取，不能将shutdown调用直接描述成同步释放OS watch。Drop同样cancel，再随字段销毁。
- VcsDirs初始化时git2 discover git_dir、ancestor find_sl_dir；即使Sapling开关关闭也保存sl_dir以排除内部文件，但只在启用时读Sapling锁/parent。不动态重新发现后来创建的VCS metadata。git classify仅GitMetaKind，Sapling无对应meta事件。
- is_internal以发现目录component前缀排除；is_lock_path仅git_dir直属index.lock/gc.pid和启用时sl_dir直属wlock，config.lock/HEAD.lock/ref锁不合成operation。lock_present用exists，未判普通文件或PID活性；两VCS取OR，gc.pid残留不会在这里判失效。
- read_head把.git/HEAD原始文本和Sapling p1 hex以|连接；不解析符号ref指向的commit，因此同branch新commit可保持相同token。两VCS均缺失返回None，有目录但读失败则空segment；从可读到不可读也可产生token变化，不能从注释推断读取失败必定head_changed=false。
- read_sl_parent只读dirstate首20字节并hex，不解析其他布局；先symlink_metadata排除非普通文件再File::open/read_exact，检查与打开分离，不能保证竞争换入symlink/FIFO时不阻塞。Git HEAD read_to_string未做普通文件/长度检查；这些同步I/O发生在async事件循环，网络FS没有spawn_blocking隔离。
- 事件循环初始Idle并读取last_idle_head，不在启动时主动drive已有锁。select biased顺序cancel、raw recv、timer，raw持续就绪可推迟settle/cooldown timer；Locked/Idle没有定时器，stale warning仅raw事件处理后检查，完全静默的长锁不主动在60秒报告，漏掉释放事件也无周期poll恢复。raw channel关闭直接结束，无Completed补发。
- process_event每批先读HEAD/当前锁，或批内精确锁路径事件作为锁活动；进入操作用last_idle_head而非已变化的batch-time HEAD。锁事件到达时文件已消失则立即Locked=>Settling，仍有Started。Idle/Cooldown更新baseline；代码明确接受下次锁事件尚在debounce而settle先到期导致baseline提前更新，可能head_changed=false，消费者刷新承担恢复，不能声称本层绝对检测全部head变化。
- 先发状态transition，再处理路径；Git metadata kind排序去重，Idle才发送，Locked/Settling/Cooldown均抑制。工作区paths保留原分组与顺序（这里不去重），Locked/Settling仍发FilesChanged，由消费者缓存；Cooldown直接丢弃文件事件，无本层补发。普通非VCS文件变化也会在cooldown丢失，不能说只丢Git生成的瞬态变化。
- timer读取新锁/HEAD驱动，Locked/Settling保操作初始baseline，Idle/Cooldown更新；timer分支不检查stale warning。所有broadcast send错误忽略，不保证无订阅者的事件后续可见。source注释的causal stream是本循环发送顺序，非OS事件完备性或可靠持久化。
- 已读测试使用temp fake.git+直接process_event，覆盖Idle文件/meta、Locked和gc.pid抑制、Cooldown丢文件、内部路径排除、lock存在性；尚未补完后续Sapling/快速操作/共享runtime等测试，当前不报告动态结果。

## Source测试收口与watcher前段

source.rs 1498行全部读完；watcher.rs推进280行。

- source后续测试覆盖5次连续/批内已消失index.lock合并、未变HEAD完成false、其他.lock不合成operation；通过直接process_event+expire_settle调用和fake文件，不启动真实Git rebase或真实timer。Sapling测试固定20字节p1零填充、短/缺/静态symlink拒绝、两VCS组合token、kill switch、内部路径锚定、无wlock的dirstate改写无事件、goto模拟与不变p1完成；没有实际Sapling CLI/部署版本布局验证。source测试并无shared runtime生命周期回归，该范围不能从测试注释或生产意图推导。
- watcher map_event_kind映射Create/Remove、Name Modify=>Renamed、其他Modify=>Modified，Access/Any/Other丢弃。原始配置默认100ms/空ignore。
- watcher早期Git whitelist与source component分类不同：用lossy路径字符串contains(.git/index、HEAD、FETCH_HEAD、refs/、packed-refs、gc.pid)，Sapling只contains(.sl/wlock)，仅正斜杠；不是精确名字，可能接受带后缀名字，Windows路径表示行为仍须实际调用路径确认。不要把这一层描述为严格source分类。
- Sapling默认开，只有env精确0/false关闭；strategy精确1/true强制PerDir，0/false Fanout，其他Linux PerDir其余Fanout，不trim。预算env parse usize且>0，失败/0默认49152，不读取实际系统inotify剩余额度。
- GitignoreCache逐路径parent向上到文件系统根查.gitignore，没有watch-root止界。遇.git祖先时watch_vcs且whitelist放行否则忽略；Sapling开时同理.sl。先命中的更深ignore立即true、更深whitelist立即false，浅层不再覆盖；path.is_dir来自处理时磁盘，已删目录可能按文件评估。缓存key为.gitignore路径+mtime，mtime相同不重读，add错误忽略/build错误empty，cache不主动清除；该逻辑不能当成git的完整ignore配置实现（全局excludes/info等此处未读取）。
- merge_events把非rename按路径HashMap合并：任意Removed胜出且后续Created/Modified不会恢复，Created+Modified仍Created，Modified+Created升级Created。Create后Remove并非取消，Remove后Create仍Removed。按kind分组也用HashMap，所以输出组及组内路径顺序不稳定；rename完全绕过合并，保留其每项原paths顺序，但统一附加在所有非rename组之后。不能承诺跨kind与rename保留原始OS因果顺序；rename路径也可能同时出现在普通组中。

## Watcher布局与增量维护（推进至930行）

- FsNotifyHandle Drop显式发送Shutdown再join线程，callback另持sender使仅drop sender不够；join无timeout，错误忽略。watch_count是Relaxed atomic的watch调用口径，fanout递归底层实际descriptor数不由该计数穷举。
- 自定义glob将无**/或/前缀的pattern补**/，首!分到include集合；非法patternwarning跳过，构建错误None。include匹配优先ignore，与原pattern顺序无关；仅custom层覆盖，不强制重新包含被ignore walker剪掉的目录。
- 初始化默认timeout10秒；fanout初始非忽略top-level数量上限64，超过则选择递归root（选择/启动完整分支待读）；StartProgress时间线最多32项。
- scan_per_dir_updates按处理时symlink_metadata判目录，普通目录加入add，Create/Remove/Rename的仍存在目录也加入prune以重新arm；非目录/丢失/symlink加入prune。Modified目录也add，结构类型并非唯一目录维护触发。
- find_git_dir ancestor扫描：真实.git目录直接返回canonical或原值，不要求合法git repo；文件/symlink才git2::Repository::open验证。source的git2 discover与此真实目录分支并非等价；find_sl_dir仅真实.sl目录、不支持文件间接引用。lstat与canonical/open分离，没有并发路径替换原子保证。
- ignore_walker和pruning_walker显式开启.gitignore/.git/info/exclude/global/.ignore、hidden(false)、不follow_links；与事件过滤GitignoreCache范围不同。pruning还剪掉任意层名为.git/.sl目录与custom忽略目录，文件留给调用方过滤。错误entry flatten跳过。自定义negation不穿透已剪掉的父目录。
- top-level筛选只真实目录、排除.git/.sl、custom匹配，超过max立即None；PerDir全树先收集到Vec再按component深度稳定排序，预算不限制遍历本身时间/内存，same-depth沿walker原序并非确定词典序。
- per_dir_git_watches为gitdir非递归、存在refs非递归及存在heads/tags递归；不递归refs/remotes或objects/modules。真实Git worktree只有其独立gitdir则可能没有refs，不能宣称共同gitdir所有refs有覆盖；heads/tags递归调用也可能消耗多个inotify descriptors。
- fanout reconcile重新选择全部top-level（这里不再用64 cap），HashSet diff顺序无保证，先add再remove，新增watch失败warning；只结构事件触发，ignore编辑不主动reconcile，新增树无文件backfill。后续目录增多不在此自动切换recursive-root。
- PerDir pending每轮最多arm512，失败debug且弹出不重试；同步arm上限4096，超出则允许ready后后台arm，深层有启动盲区。真正ready信号与预算计算分支仍待读。
- add_subtree_watches先拒symlink/消失/已watch根，再pruning walk按目录出现时watch，非目录depth>0且custom通过则分批512发synthetic Created。watch失败仍继续walk/backfill；非目录分支不限制普通文件，可能包含symlink路径；无通道背压。budget以watched.len达到后break全部剩余walk，不只跳最深节点；每次动态调用可warning。注释“never lost”依赖成功监听/遍历/未触预算等条件，不能作为无条件契约。
- prune_subtree_watches component前缀找全部记录，unwatch错误忽略仍移除；没有等待读线程队列pending也被同步剪除的实现，此部分要继续查command loop。

## Watcher生产逻辑完成（推进1510行）

- start_with_timeout创建无界raw/command通道，canonical cwd失败用原路径。NoCache禁用全树file-ID缓存，rename可能分裂Remove/Create；notify不follow symlink，但不等于拒绝所有被明确发现的外部gitdir。
- callback先merge后逐path过滤，custom include优先返回true，甚至绕过GitignoreCache/VCS whitelist；否则gitignore后custom ignore。此前walker层custom include不能穿透剪枝，事件层却能覆盖gitignore，两层并非完全相同语义。过滤后才生成维护候选；被忽略路径不触发reconcile。发raw事件先于批末command更新，synthetic backfill稍后由另线程发送，无统一OS时间排序。debouncer错误仅warning，没有补扫/rescan信号。
- PerDir全树选择截断到budget（不计root和VCS额外watch），top-level先同步arm，深层pending<=4096全部同步，否则全部留后台。root watch失败返回WatcherStart；child/VCS失败只warning仍ready，pending失败debug仍ready，不承诺返回成功代表全覆盖。fanout<=64使用非递归root+递归child，否则递归root；后者仍由callback过滤事件但不会省掉内部树OS递归成本。
- VCS发现后只设置一次surgical/recursive watches，git/sl watch失败继续；refs/heads/tags后来出现没有专门新增watch分支，PerDir Update反而排除git_dir/sl_dir下add，所以不可保证后来新建命名空间全部递归覆盖。预算未计VCS递归内部descriptor数。
- command loop drain到Empty才arm512，因此持续命令流可推迟pending；Update先prune再add，prune只在watched_dirs.contains候选根时执行，不包含pending目录。add排除root和发现VCS内部，不含额外root containment check。动态add按watched.len预算，但后台arm_pending_chunk不重新检查预算/ignore，pending可在更新后继续arm，不能把budget当严格总上限。
- watch_count保存1+watched_dirs.len+vcs_watches，不包含pending，也不表示递归底层descriptor实际总数；退出前仍写最后值而非0。source.shutdown只cancel async loop，OS线程继续直到handle Drop；Drop send Shutdown并无界join，可能等待在途同步walk/IO完成。
- ready等待recv_timeout；Timeout和ready通道Disconnected均映射FsNotifyError::Timeout，记录有界stage timeline并enqueue Shutdown，JoinHandle丢弃不join。慢初始化要完成后才处理Shutdown，timeout不是立即终止线程/遍历的硬deadline。显式初始化Err返回WatcherStart，线程自然结束，亦不在此join。
- watcher生产部分全部覆盖；测试入口重试最多3次，每次15秒，失败sleep100ms，不能把其成功当作首次启动可靠性。后续真实FS测试1511–3885尚待读。

## Watcher真实FS测试前段（推进2500行）

- collect_events_smart在总deadline内try_recv，每10ms sleep，有事件后quiet period到期提前返回；default300ms/quiet50ms。这是有界采样而非保证所有延迟事件到达。
- 已读创建、删除、快速多建、negation、git内部、建目录、info/exclude top-level排除、recursive fallback、新top-level/moved-in/删除重建、symlink root动态、PerDir ignored和backfill测试，大量显式#[ignore]，默认Cargo不会执行。活跃modify接受Modified或Created，rename接受任意Renamed或新文件Created/Remamed，不要求rename旧新配对；深层创建有正向文件断言。
- test_debouncer_gitignore_respected的否定检查使用Path::ends_with(".log")，这是路径component匹配而非扩展名，对debug.log不能有效识别，属于断言缺口；正向readme.txt仍可证明监听活性。custom *.tmp和nested规则测试使用完整文件名，没有此特定缺陷。当前迁移只记录，不改运行代码或测试。
- handle Drop测试在独立线程5秒内join并随后2秒内观察raw channel断开，再写文件检查不再收到；这确证小临时目录场景退出，不为生产Drop增加timeout。
- fallback/info-exclude测试意在证明Fanout超过64退recursive后忽略目录可产生事件，调用start_with_retry使用平台默认strategy，未强制Fanout；Linux默认PerDir时该预期不一定适用，而且该测试ignored，不能将其作为Linux默认已验证行为。
- PerDir watch_count测试显式strategy且非ignored，构造ignored嵌套树，期待root+2workspace+3VCS=6；验证公开计数口径，不读取系统inotify descriptor真实数量。后续new subtree backfill测试已读至early.rs断言，尚未完成其余部分及更多测试。

## Watcher全文件阅读完成（3885行）

- 补完PerDir backfill和删除子树测试，两者ignored；前者检查early/late文件与watch_count增长，后者只比较count下降，未逐个验证系统watch身份。
- merge测试覆盖重复Create、Create/Modify优先、Remove、分组及过滤。test_merge_any_then_rename把输出再次收集为HashMap，同路径后置rename覆盖前组，故不能证明实际输出只剩Rename；源码实际保留两种事件，此断言弱于注释。没有Removed后Created和跨kind排序的完整断言。
- glob测试覆盖空集、include/ignore、目录、绝对和已有**/前缀、brace pattern；绝对pattern测试只断言/root_only.txt本身，不证明注释的所有后缀路径匹配。cache测试验证whitelist与Sapling开关路径，但未动态变更mtime缓存测试。
- selector测试覆盖一级目录、隐藏目录、.git/.sl排除、gitignore/info-exclude与custom优先、被祖先gitignore忽略的显式watch root仍可选子目录、静态symlink跳过。显式root可选择子目录不等于事件GitignoreCache放行同一路径，后者仍向上查祖先规则。
- PerDir selector验证嵌套剪枝、浅层优先、custom、symlink、Git精细watch和worktree无refs形态。scan_updates测试是on-disk候选分类，不建立真实watch，不能替代删除重建内核descriptor重新绑定验证。
- helper测试覆盖.git实仓库/祖先/合法gitlink、非法gitlink与外部非Git symlink拒绝、.sl祖先/静态symlink拒绝、独立VCS watch选择、Sapling env开关、fanout cap等于值包含与ignored不计入、HashSet差分。external_ancestor_sl_arms_in_recursive_root_mode只验证布尔选择，名字中的arms不代表实际watch调用。无repo测试允许发现树外ancestor repo，不强制None。
- Sapling env测试通过Drop删除env恢复，并不保存调用前原值；计划动态测试使用独立测试进程，串行避免本包进程内争用。watcher全文件没有初始化timeout退出或大规模pending预算竞争的真实回归；这些仍仅源码事实，不能由其他小目录测试推广。
- 尚余tests/integration.rs、benches/startup.rs、examples/watch_stats.rs，完成后再执行本包测试和规范映射。

## 独立集成测试、benchmark与示例（全部阅读）

本包全部10份Rust文件（6701行）和manifest已读，待测试终态与能力映射。

- integration8项：6项公共source真实OS事件测试ignored，模拟Git/Sapling目录和锁，不运行对应VCS命令；recv_until忽略Lagged，不能验证无丢失。2项活跃shared测试首次OS watcher不可用时打印skipping并直接return成功，必须查nocapture日志才能排除静默跳过。顺序调用验证Arc同一对象、不同目录、释放后重建以及50次调用1created/49reused，不是并发竞争或跨runtime退出测试。
- startup Criterion包含favorable、48带target、64无ignored、400wide、nested ignored五种树。实际--bench规模12000、sample_size30；非bench smoke规模1200且width改为12/16/65。树创建在计时外，iter_batched只测start到ready，Drop不计时，runtime仅enter不运行async事件循环；不测事件吞吐、完整后台arm完成或退出延迟。名称fanout不强制策略，须env明确A/B，Linux默认PerDir。没有运行真实性能benchmark，不能报告性能收益。
- watch_stats示例gen js/large会在用户指定目录create_dir_all并覆写.gitignore、模拟.git/HEAD/index等文件，没有空目录检查；这是人工性能辅助，不是安全合并到现有项目的生成器。run默认3轮，启动current-thread runtime但不block_on，记录ready和轮询inotify数量估计armed；Linux读取/proc/self/fdinfo所有inotify wd行，非Linux返回0（非不存在watch）。连续一次250ms计数不变即认为armed，或120秒截止，不能证明后台pending完成；输出最后一轮count和排序len/2中位数，iters0会索引panic。walkdir_count不follow目录symlink且错误忽略，不是精确完整性验证。

## 收口

全包已读；macOS默认串行测试119通过、21忽略、0失败，shared测试未条件性跳过。

## 功能与规范映射

- [Filesystem semantic event protocol](../specs/filesystem-events/spec.md#requirement-filesystem-semantic-event-protocol)：FsEvent SHALL 使用type/data snake_case表达文件组、Git元数据、操作开始和完成；文件组共享Created/Modified/Removed/Renamed一种kind。
- [Filesystem source configuration and startup](../specs/filesystem-events/spec.md#requirement-filesystem-source-configuration-and-startup)：FsConfig SHALL 默认100ms debounce和空ignore；start要求当前Tokio runtime，start_on使用传入handle启动异步循环。
- [Filesystem canonical shared source registry](../specs/filesystem-events/spec.md#requirement-filesystem-canonical-shared-source-registry)：shared SHALL 以canonical路径Weak缓存存活source，首次配置生效；并发创建在registry锁外完成再选择已有赢家。
- [Filesystem shared runtime and statistics](../specs/filesystem-events/spec.md#requirement-filesystem-shared-runtime-and-statistics)：共享循环 SHALL 优先首次注册的runtime handle，否则当前runtime；stats剔除无strong引用条目并返回创建/复用计数。
- [Filesystem subscription and shutdown lifetime](../specs/filesystem-events/spec.md#requirement-filesystem-subscription-and-shutdown-lifetime)：订阅 SHALL 使用容量256 broadcast独立backlog；source.shutdown只cancel异步循环，Drop再随handle销毁OS线程。
- [Filesystem VCS discovery and internal paths](../specs/filesystem-events/spec.md#requirement-filesystem-vcs-discovery-and-internal-paths)：source SHALL 初始化时发现Git与Sapling目录，以component前缀剔除内部路径；Sapling关闭仍排除发现的内部目录。
- [Filesystem VCS lock and parent sampling](../specs/filesystem-events/spec.md#requirement-filesystem-vcs-lock-and-parent-sampling)：锁采样 SHALL 检查Git直属index.lock/gc.pid和启用时Sapling wlock；HEAD token连接原始Git HEAD文本与dirstate前20字节hex。
- [Filesystem merged operation state machine](../specs/filesystem-events/spec.md#requirement-filesystem-merged-operation-state-machine)：锁操作 SHALL 释放后等待500ms settle，期间重新加锁保留最初HEAD和起始时间，不重复Started；到期无锁才Completed。
- [Filesystem stale lock warning semantics](../specs/filesystem-events/spec.md#requirement-filesystem-stale-lock-warning-semantics)：StaleWarn SHALL 对Locked或Settling跨合并操作严格超过60秒仅报警一次，离开两状态后重置。
- [Filesystem event loop operation ordering](../specs/filesystem-events/spec.md#requirement-filesystem-event-loop-operation-ordering)：事件循环 SHALL biased优先cancel再raw再timer，用last_idle_head作为操作基线；批中锁路径即使已消失仍合成Started并settle。
- [Filesystem operation event suppression](../specs/filesystem-events/spec.md#requirement-filesystem-operation-event-suppression)：process_event SHALL 先发状态transition，Idle发送排序去重GitMeta；Locked/Settling发送文件事件供消费者缓存。
- [Filesystem raw event merge policy](../specs/filesystem-events/spec.md#requirement-filesystem-raw-event-merge-policy)：raw合并 SHALL 丢Access/Any/Other，非rename按path合并且Removed胜出、Created优先Modified；rename保留paths并后置。
- [Filesystem custom glob precedence](../specs/filesystem-events/spec.md#requirement-filesystem-custom-glob-precedence)：custom glob SHALL 将无绝对或**/前缀的模式补**/，首!作为include，include优先ignore，非法模式warning跳过。
- [Filesystem ignore cache scope](../specs/filesystem-events/spec.md#requirement-filesystem-ignore-cache-scope)：事件GitignoreCache SHALL 从path父目录向上读取.gitignore，以mtime缓存，深层ignore或whitelist立即决定。
- [Filesystem watcher VCS whitelist](../specs/filesystem-events/spec.md#requirement-filesystem-watcher-vcs-whitelist)：watcher SHALL 以正斜杠lossy路径contains允许Git index/HEAD/FETCH_HEAD/refs/packed-refs/gc.pid及启用时.sl/wlock。
- [Filesystem watch strategy and budget selection](../specs/filesystem-events/spec.md#requirement-filesystem-watch-strategy-and-budget-selection)：策略 SHALL 精确env1/true选PerDir、0/false选Fanout，否则Linux PerDir其余Fanout；Sapling仅精确0/false关闭。
- [Filesystem ignore aware directory selection](../specs/filesystem-events/spec.md#requirement-filesystem-ignore-aware-directory-selection)：目录选择 SHALL 开启.gitignore/global/exclude/.ignore、允许hidden且不follow symlink，PerDir剪VCS/custom子树后全量收集浅层排序。
- [Filesystem fanout and recursive fallback](../specs/filesystem-events/spec.md#requirement-filesystem-fanout-and-recursive-fallback)：Fanout SHALL 在非忽略一级目录<=64时非递归root加递归child，超过64用递归root。
- [Filesystem surgical metadata watches](../specs/filesystem-events/spec.md#requirement-filesystem-surgical-metadata-watches)：PerDir Git SHALL 非递归gitdir和存在refs，递归存在heads/tags；Sapling非递归.sl。真实.git目录直接采用，gitlink/symlink经git2验证。
- [Filesystem staged watch arming](../specs/filesystem-events/spec.md#requirement-filesystem-staged-watch-arming)：PerDir SHALL 同步arm一级目录和VCS，深层pending<=4096同步清空，否则ready后每块512后台arm。
- [Filesystem dynamic subtree maintenance](../specs/filesystem-events/spec.md#requirement-filesystem-dynamic-subtree-maintenance)：PerDir更新 SHALL 按当前lstat判add/prune，结构事件仍存在目录先prune再add，非目录候选prune。
- [Filesystem subtree creation backfill](../specs/filesystem-events/spec.md#requirement-filesystem-subtree-creation-backfill)：新增子树 SHALL 惰性watch目录并将walk发现的非目录项分批512作为Created回填，跳过已watch根与symlink根。
- [Filesystem watcher timeout and callback failures](../specs/filesystem-events/spec.md#requirement-filesystem-watcher-timeout-and-callback-failures)：初始化 SHALL 等待ready最多配置timeout默认10秒，超时或ready断开返回Timeout并排队Shutdown。
- [Filesystem measurement utilities](../specs/filesystem-events/spec.md#requirement-filesystem-measurement-utilities)：startup benchmark SHALL 区分12000目录正式bench与1200目录smoke，仅测start到ready；watch_stats提供生成树和多轮计数测量。

## 边界

- 本层不提供；Git丰富信息与多root组合属于workspace层，枚举non_exhaustive。
- start返回NoRuntime；root/debouncer初始化失败WatcherStart，部分child失败可能仍ready，不保证成功即全覆盖。
- 存活source忽略新配置；最后Arc释放销毁，后续重新创建；canonical失败使用原路径key。
- 败者计reuse但曾实际创建watcher；live按Arc而非健康状态统计，handle注册不证明runtime仍运行。
- 落后可Lagged且无重放；OS watcher仍由handle持有，最终Drop发送Shutdown并join，没有硬退出期限。
- 不动态重发现；只精确HEAD/index/packed-refs/FETCH_HEAD及refs前缀分类，非UTF8不能分类。
- 不解析ref目标commit；失败贡献空segment且可能改变token。Sapling先lstat后open不保证竞争下文件身份，I/O同步执行。
- 变化进入500ms cooldown，不变Idle；cooldown重新加锁开始新操作。
- source只在raw事件后检查，无Locked周期poll；不强制解锁，不保证60秒自动报警或恢复。
- timer可能延迟，基线存在已记录竞争可导致head_changed=false；无事件完整性保证。
- GitMeta与FilesChanged均被丢弃，无本层补发；路径组不在此去重，send错误忽略。
- 仍Removed；HashMap组及组内顺序不稳定，rename可与普通组重复路径，不保留全部OS因果顺序。
- callback include先放行，可覆盖gitignore；watch选择已被ignore walker剪掉的目录不能借include重新进入。
- 同mtime不重载；is_dir按处理时磁盘值，删除目录可能按文件评估；本cache不读取global/info-exclude/.ignore。
- 此层不是精确component分类，不能推导全平台严格路径等价；最终source另按发现目录分类排除。
- 默认49152，parse正usize有效；不读取系统剩余额度，root和VCS额外watch不含在该子目录预算。
- 预算不限制选择遍历内存时间；root选择和事件过滤有不同ignore边界，不能推导完整事件放行。
- reconcile只结构事件触发且不再执行64cap切换；新增树无文件backfill，recursive fallback可覆盖watch层排除本可省下的树。
- 不补watch共同gitdir全部refs，不专门动态arm新增VCS命名空间；VCS失败warning仍可ready。
- 失败弹出不自动重试，ready不证明后台完成，可能丢启动事件；持续command可推迟pending。
- command不清理pending，后台arm不重验预算/ignore；预算非严格总上限。
- 失败仍可能继续walk，达到预算break剩余遍历；可重复或漏事件，无界通道，不能无条件承诺never lost。
- timeout不立即中断线程，后续处理Shutdown；callback错误只warning，无自动补扫信号。
- kernel计数为0不代表无watch，250ms一次不变仅估算armed；iters0索引失败，gen会覆写指定目录模拟文件，非无损项目生成器。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
