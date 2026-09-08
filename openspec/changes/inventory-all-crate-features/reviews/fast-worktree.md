# fast-worktree 逐文件审阅

状态：全部源码已审阅并完成功能映射；下文按阶段保留历史记录。动态测试未运行，平台与磁盘限制见末尾。

## 入口与构建边界

- 0.1.0，默认未启用metadata；default-bazel启用metadata。metadata门控SQLite数据库、discovery、GC和注册记录，Linux独立无条件引入serde以支持overlay。bench feature开放pool-perf-bench及tempfile；普通fast-worktree CLI不需要此feature。
- lib暴露builder、删除与批量清理、sync及dirty state、snapshot/ref转移和stale registration清理；Linux另暴露btrfs/overlay orphan cleanup与git state cleanup；metadata再开放auto-GC与DB。不将仅某feature/OS可用的API写成全平台默认能力。
- count_tracked_files调用gix discover与index_or_load_from_head后entries.len，不自行walk，但注释O(1)不能证明整个入口恒定IO/CPU，索引加载可能随规模变化。

## builder与计划

- 同步阻塞API。默认HEAD、Linked、PreserveWorkingTree、Ignored Skip、parallelism0、ignored_parallelism0、channel_buffer256、新CancellationToken；构造setter未验证输入。standalone(false)不复原已有Standalone设置。
- CreationMode数据库名称linked/standalone/git；WorkingTreeMode三个分支与Ignored Copy/CopyOnly/Skip是显式计划参数，实际组合行为待execute核对。BtrfsDelegate要求Send+Sync，create/delete抽象调用；mount/unmount overlay默认报unsupported，不是所有delegate都支持overlay。
- create先execute_plan再把typed StorageFull或错误链任意包含英文ENOSPC消息的错误顶层加not enough free disk space（跨crate字符串契约）；保留原链。仅create调用此annotation，copy_ignored_only无同样处理。
- metadata仅设置kind时创建后调用register_worktree，先有磁盘结果再登记；登记函数细节待读。CopyReport保留非致命issues及可选dirty报告，统计不自动等同完整复制成功。
- copy_ignored_only不创建/最终化worktree，即便IgnoredFilesMode::Skip也执行ignored复制（skip patterns为空）；worker数优先ignored_parallelism、其次parallelism、再CPU数。先收集unignored路径作skip_files，再respect_gitignore=false复制剩余内容；copy_parallel返回部分成功时检测取消并报错，不回滚已复制文件。
- WorktreePlan的effective_parallelism取显式值否则CPU数；effective_ignored_parallelism仅ignored值否则CPU数，并不继承parallelism，与copy_ignored_only入口不同。

## 删除与清理入口（api至1000行）

- 删除顺序Linux overlay live mountinfo/metadata、btrfs metadata恢复、btrfs探测，随后通用目录删除。只有on-disk函数返回成功才调用DB注销；成功是否代表所有清理完成仍受best-effort边界限制。
- 通用路径先读.git注册指针，再symlink_metadata：symlink unlink，其他类型remove_dir_all，任意metadata错误视为不存在。注册目录删除错误忽略；read_worktree_gitdir读取整个UTF8文本、trim后精确gitdir: 空格前缀，相对路径join worktree，canonical失败仍保留原路径，没有在此验证其属于.git/worktrees。由此不能将该公有删除入口当受限worktree身份验证器。
- cleanup_worktrees_in扫描一至二层：第一层任意symlink直接尝试删除；目录有.git则删除，否则遍历其所有子目录或symlink，不要求第二层.git存在，再best-effort移除父空目录。read_dir/entry/metadata错误多为忽略，不累计errors；计数仅来自单项remove的Result。
- Linux overlay wrapper按live mountinfo再持久metadata识别；具体安全和挂载行为待overlay文件阅读。btrfs direct delete失败有delegate就尝试，delegate失败仅日志并返回原始direct错误。
- Linux symlink目标解析相对parent；检测btrfs后用is_safe_snapshot_delete_target检查，unsafe只尝试unlink并返回成功（unlink错误忽略）。safe先删除snapshot再metadata及symlink，后两者best effort；delegate成功直接返回其report，local清理由delegate负责。检查到按path删除间仍有TOCTOU，代码注释已承认，不能规范成完全原子安全边界。
- symlink读取失败或非btrfs目标先best-effort unlink再回退；可能忽略unlink错误。直接/bind snapshot删除逻辑从1001行继续，尚未完成审阅。

本次只提取事实，潜在债务单列，不修改运行时代码。

## API 删除恢复与GC（1001–2110行）

- 直接bind btrfs路径先运行detached umount，spawn错误传播，退出非零只warn仍尝试snapshot删除；成功删除后metadata清除best effort。delegate成功返回其report，不合并本地已unmounted状态。
- metadata恢复枚举当前btrfs mount下指定存储目录，按suffix读取JSON，坏文件跳过，mount_target精确Path相等。snapshot必须直属扫描目录且safe predicate通过；但非symlink target的umount在snapshot是否contained之后计算、真正拒绝之前执行。refused时保留metadata，却仍best-effort移除target引用并返回Some成功；上层可注销DB。目标不存在时也清metadata并报成功，不能以RemoveReport证明完整清理。
- orphan扫描坏/不可读metadata会尝试直接删除且不计errors；live判定为任意mountpoint精确匹配target或symlink lexical解析等于snapshot，无canonical规范化，所以等价含..路径可能不匹配。target父目录不存在则跳过以避免restore前误删。
- orphan对存在且不contained的snapshot记errors并保留；其他分支先unlink/umount引用，再delete snapshot（与手动删除的先snapshot后引用不同）。snapshot删除失败保留metadata供重试；无delegate参数。umount、target删除、metadata删除错误可忽略，removed增加不保证所有动作成功。
- register_worktree默认DB打开/写入失败仅warn；对worktree/source canonical失败回原路径，ID可自定否则path派生，存Alive、creator pid、now、可选session/metadata及请求creation_mode/ref。unregister发生磁盘删除后再canonical，此时可能失败回原路径，不能保证与登记canonical路径匹配；注销错误忽略。
- GC选项优先skip_kinds → per-kind map → global age；None为不因age删除，force不覆盖never-expire。age_path_enabled只看global Some或map非空；全None map仍会扫描。
- Unix pid0及>i32MAX判dead，kill0只有ESRCH判dead，EPERM/EACCES等视alive；nonUnix所有pid判dead，因此并非活跃性保护的fail-closed。PID复用无start-time校验。
- Linux枚举/proc numeric目录并忽略单pid读取失败；macOS libproc逐pid失败同样跳过。最终要求观察到自己当前cwd，否则Failed；这只证明自己可见，不保证扫描覆盖所有外部进程。Unsupported允许age路径（仅可用PID保护），Failed非force阻止age路径。
- macOS固定1024字节vip_path要求存在NUL且非空；list_all_pids实现把probe和fill返回值都当byte count除以4，最多4次增长重试。该API单位的注释尚未经系统实现验证，不能将注释当事实；后续动态测试/本机SDK核对。
- cwd保护通过raw/canonical路径starts_with判断子目录；last_active=max(created_at,last_accessed_at或created_at)，age负值clamp0，严格早于now.saturating_sub(age)才过期，精确边界保留。
- dead路径dry_run只读计数dead或!exists；真实先sweep_dead，再删除DB记录（unregister错误折为false）。dead清理不遵守skip_kind/protect/force活跃性保护，且不执行磁盘清理。
- age路径force不扫描CWD且跳过PID/protect/fresh记录重查；never-expire仍遵守。普通路径先读alive列表、过滤过期与guards；dry_run只计exists并不执行fresh重查。实际删除前重新get_by_id和扫描CWD，扫描失败直接停止剩余age项。
- fresh判断使用新记录的kind/age/path等，但最终remove仍使用初次rec.path；没有事务锁覆盖recheck到删除，不能宣称race-free。is_reclaimable本身不检验status；取得不到fresh或DB错误直接跳过无计数。删除调用默认DB注销，而gc可接收任意WorktreeDb，待DB测试核对一致性。
- 已进入GC测试，读取至2110：errno分支、真实PID与reaped child、supported OS真实CWD、自身扫描缺失fail-closed、NUL边界、嵌套canonical路径及reclaimable部分。其余测试仍待阅读，当前未运行。

## API 测试阅读（2111–3160行）

- 完成GC私有predicate测试：kind优先级、skip胜过map、never-expire、per-kind age覆盖全局、访问时间及保护路径。证明这些纯函数选项分支，不证明生产扫描覆盖所有进程或并发删除安全。
- 错误链测试覆盖StorageFull、Unix ENOSPC28、Windows112/39（cfg门控）、英文文本、无关错误与顶层context保留原链；不是实际写满磁盘的端到端测试。
- builder链与standalone shorthand仅构造不读字段/执行，variant相等测试不证明创建模式行为；report测试只做字段映射。copy_ignored_only取消测试仅预先cancel，不测试复制中途、清理或取消延迟。
- plain/nested worktree清理测试创建真实临时Git仓库，检查目录删除与报告；require_git可能在不可用时提前跳过。dangling symlink包括一层/二层，用symlink_metadata确认指针已删除。
- delegate-aware plain linked test额外检查.git/worktrees注册目录消失及delegate计数0，有真实删除结果；metadata下使用GrowHomeFixture隔离。其他更早清理测试无局部GrowHomeFixture，运行全metadata测试应提供进程级临时GROW_HOME以隔离默认DB注销副作用，不能用真实用户DB。
- Linux delegate fallback测试注入成功/失败closure和RecordingDelegate，只证分派与计数；不实际执行btrfs或权限服务。
- metadata public writer到removal/orphan scanner测试用伪MountEntry、临时目录及不存在snapshot，实际只验证metadata可发现与引用清理，btrfs delete被门控。legacy空目录路径会尝试umount但没有真实挂载；不能将成功解释为内核卸载已验证。
- active mount测试纯mountinfo夹具，active symlink测试真实普通目录symlink，缺父目录测试确认skip无error且snapshot/metadata保留。symlink_resolves_to只测直接绝对与简单相对路径，无含..的等价目标。
- non-btrfs symlink测试确认只unlink不删除真实目标；metadata无匹配返回None。当前读至metadata_integration开头3160行，余下1513行测试及其他模块仍待阅读。

## API 全部阅读完成（3161–4673行）

- metadata登记测试实际走register_worktree/open_default，检查canonical路径、kind/session/creation/head/pid；unregister_worktree_removes_by_path名字虽像入口测试，却直接调用db.unregister_by_path，不覆盖删除后canonical失败的问题。
- dead记录与dry-run检查DB行及Alive状态，missing且expired只计一次；force liveness测试只看计数不查实际DB删除。部分age测试使用自定义db_at且调用生产默认DB注销，仅检查目录/计数，未证明传入DB中的expired行当轮消失。
- gc_with_delegate明确让GROW_HOME等于传入DB路径，检查目录和记录消失以及delegate未调用。此条件回避了自定义DB不等于默认DB的边界，不能证明所有WorktreeDb入参都被同步注销。
- 真正的CWD guard集成spawn sleep30放在worktree子目录，最多200次10ms等待scan观察子cwd，首次GC保护；kill/wait后等待scan不再包含，再GC确认删除。有真实跨进程验证，但assert失败前child无RAII kill guard，可存活至30秒；不检验扫描到删除间并发chdir。
- gc_age_path_fail_closed_when_scan_unusable仅直接调用predicate，不注入生产扫描失败；gc_protect_paths_pre_remove_recheck从首次过滤就保护，实际不会抵达fresh recheck，因此名称不能证明第二次检查路径覆盖。
- failed removal用regular file触发ENOTDIR，无需权限假设，检查remove_failed、目录项与DB保留；默认DB成功路径真实Git worktree确认磁盘与记录都删除。失败路径与默认DB成功路径证据有效，但不覆盖metadata返回成功却未清理的Linux分支。
- per-kind集成覆盖skip优先级、force不覆盖never、manual显式可过期、仅map启用age、subagent短TTL与session长期、dry-run按kind计数。serde覆盖选项/报告/DbStats，旧字段缺省与map null为None。没有新运行时代码变更。

api.rs全部4673行已读。仍需worktree执行、copy/git、sync、auto_gc、DB/discovery、Linux挂载模块、bin与integration逐文件审阅，之后才运行整包验证并映射。

## execute.rs 1–1120行

- PartialWorktreeGuard drop调用无delegate remove_worktree，错误忽略；join_git_copy把线程panic与内部错误转anyhow。guard仅在明确创建后设置，并非所有创建前/线程启动失败路径已有保护。
- snapshot清理仅.git为目录时进行；固定top-level transient项及所有非objects子树*.lock普通文件best effort删除。目录递归先收集完整Vec，entry/file_type错误忽略，不跟随目录symlink递归；path.is_dir/is_file仍可跟随symlink，.git本身也可为symlink。返回删除计数，不保证没有残余锁。
- 成功创建后best-effort写.git/grow-worktree-source；仅独立.git目录，已有marker原样保留，source根路径lossy字符串无newline；不验证继承marker内容是否仍对应最终主仓库。
- Linked与Standalone在Linux都先overlay、再btrfs、再各自copy；GitCheckout不走snapshot。fast path错误warn后回退，skipped_reasons仅日志，最终copy错误没有代码注释所称附加全部fast-path上下文。
- overlay只有确定Private且无delegate才跳过；Unknown允许，delegate可在Private启用。snapshot返回copy stats全0/ignored None/dirty None，实际已包含全部源文件，不按Ignored Skip或skip_patterns删ignored；Linked模式快照也不保证.git为linked注册形态。
- finalize_clean_and_ref先按working mode清理，再必要checkout非HEAD ref。overlay pristine判upper顶层仅.git且无staged就跳过reset/clean；read_dir根错误true，但entries.flatten忽略单项读取错误，可能错误判pristine。CleanTracked条件reset不清untracked，CleanAll额外clean -fd不含ignored；Preserve另ref可能checkout失败。此snapshot分支未观察CancellationToken。
- overlay创建后清git状态和finalize失败才reclaim，reclaim失败忽略；finalize_or_reclaim仅Result Err，不覆盖panic。btrfs direct探测error直接传播外层fallback，None或root不等转delegate；execute失败也转delegate，无delegate时原direct错误被吞为None。
- reclaim_btrfs_snapshot即使delete失败仍移除metadata和exposing symlink，可失去恢复追踪。delegate create失败降级None；成功信任returned worktree_path执行finalize，但report路径及delete参数用plan.dest，未检查二者一致，也不本地clean snapshot git状态（依赖delegate约定）。
- linked copy从source git root复制，git add使用原source及lossy dest字符串；add成功才装guard，随后检测cancel、扫描modified、按Clean模式skip dirty、copy后再次检测cancel。Preserve复制源index并仅更新clean metadata stat；CleanTracked hard reset，CleanAll再clean -fd。
- linked ignored Copy阶段以已复制路径为skip集合、respect_gitignore=false再次copy；此前被Clean模式跳过的dirty路径不在copied集合，潜在再次复制范围需结合engine确认。此阶段token move后未再检查cancel，copy_parallel若以部分Ok结束则可能成功disarm guard；CopyOnly在create中视同Skip。
- standalone create_dir_all允许已有dest，随后spawn .git复制；至1120行期间Preserve扫描线程spawn错误或Clean get_modified_files错误直接?，尚无guard及join .git线程，可能留下后台写入的部分dest。后续copy/join/finalize从1121行继续阅读，不提前声称全路径清理保证。

## execute.rs 全部完成，worktree/mod.rs 至100行

- standalone在copy返回后无条件join .git线程，再依次处理copy错误、git错误和取消，失败best-effort reclaim；但此前扫描/线程spawn的?路径仍绕过此join。copy和git同时失败时只返回copy错误。
- Preserve实际join modified scan并同步update_index_stats，不是函数注释所称fire-and-forget；guard覆盖此后finalize/checkout/commit Result错误。非HEAD checkout发生clean之后；HEAD报告commit从source读取而非dest，源并发移动时可能与已复制git不一致。
- standalone ignored阶段同linked，无copy后cancel再检查；从已copied路径做skip，读取范围需后续engine确认。错误reclaim可删除已有dest，入口create_dir_all不拒绝非空目的地，属于调用方必须理解的当前边界。
- GitCheckout使用git -c checkout.workers=N worktree add --detach，因此api注释single-threaded不准确；实际无token检查、无PartialGuard，忽略working_tree与ignored参数，仅checkout ref后获取commit。git成功而get_head失败不自动回收。
- snapshot组合器测试仅closure，reclaim_btrfs测试用普通目录，断言symlink/metadata消失却不要求snapshot删除；它恰好不能证明无磁盘泄漏。delegate实际Git错误测试只验证delete调用1次，RecordingDelegate不真正清理且不核对删除参数；成功测试只Some/count0，未验证report路径与delegate路径一致。
- cleanup测试验证多层lock/transient删除、保留核心文件、跳过objects、linked no-op；没有权限失败/目录symlink破坏范围用例。marker成功测试只检查记录路径下存在.git而非精确等于source，已有marker测试明确保留任意原文。
- worktree/mod.rs生产逻辑只结果结构与execute转发，1–100行已包含simple及parent目录测试；二者检查文件exists与commit非空，不检查内容/准确commit。剩余100行之后测试待读。

## worktree/mod.rs 全部1317行阅读完成

- ignored Skip/Copy、copy-only默认Skip仍复制、glob跳过、Unix symlink（文件目标精确检查、目录仅类型检查）有临时仓库断言。多数以exists为准，内容另有Unicode、0..255二进制、空文件完整比较；权限只检查任一execute位，不保证0755完整mode。
- dirty Preserve检查实际内容与porcelain包含路径，但不检查精确XY staging列；若git失败空stdout，clean-only断言可能通过（若干status命令未检查exit）。CleanTracked检查original内容，CleanAll检查untracked缺失；其ignored缺失来自默认Skip没有复制，注释称被CleanAll清理并非对clean -fd行为的证明。
- test_git_status_is_instant只记录两次耗时及trace警告，无性能失败阈值，实际上只断言stdout为空；background_finalization只是文件存在，不证明后台或index完成。
- BTRFS集成Linux下无可用subvolume早退；即使找到，创建的是subvolume内普通repo目录，不保证source本身subvolume。允许copy回退，甚至创建Err只打印不fail；因此该测试通过无法证明snapshot创建成功。它用PID路径并预先删除旧目录，未来Linux执行须确保BTRFS_TEST_PATH为明确隔离区；当前macOS不会编译该测试。
- standalone独立性检查git log成功及源worktree list无standalone，没有删除source后再验证；rename promotion只是移到新promoted路径，非原子替换源仓库测试，但确实核对rename后HEAD与报告commit。工作树历史注册不复制测试有目录存在前置。
- pre-cancel linked确认dest和注册目录清除，注册read_dir失败折0；standalone确认dest消失，但无延迟注入证明后台join顺序。hard error linked使用非法ignored glob，standalone使用坏ref，确认guard安装后的清理，不覆盖guard之前扫描/线程spawn失败、ignored复制中途取消或snapshot取消。

worktree目录三个文件已全部读完，仍需copy/git/sync/auto_gc/db/discovery及Linux模块、bin/integration。

## copy 七文件完整阅读（gitdir除外）

已完整读取mod15、types77、shard90、cow117、skip112、worker135、engine442行及其中测试。

- shard按parent路径rapidhash v3模worker数，Unix原始bytes、其他lossy UTF8；short_path_hash为全64bit十六进制，不canonical不防碰撞。num_shards0会取模panic，engine正常选择至少CPU数；同目录同shard测试有效，不同目录测试只不panic，未验证负载均衡。
- CopyStats merge普通u64加法无saturation；issues无上限。clone_file委托reflink_or_copy后从source metadata复制permissions，错误传播到worker再降为issue；并非全部metadata/ACL/xattr保证，也无临时文件原子发布。测试不区分CoW/普通copy；权限测试使用Unix扩展但缺cfg(unix)，Windows cargo test可能编译失败。
- replace_symlink先忽略remove_file错误再创建，非原子；现有目录无法由remove_file替换。Windows一律symlink_file，不区分目录目标。Unix测试有效证明普通文件可替换为dangling symlink。
- unignored扫描hidden false、gitignore true、global/exclude false，跳所有名.git entry；忽略walk错误并返回Ok部分集合，未consult Git index，所以关闭info/exclude不等于对所有tracked文件可靠。仍沿用ignore库其他默认规则（如.ignore/parent行为），不能描述为纯git ls-files分类。glob非法立即报错。
- engine workers自动/显式都cap macOS8其他32；每shard bounded channel，buffer0为同步传送。Walk同worker数并跳.git，respect_gitignore可配但global/exclude恒关。skip集合或glob命中返回Continue，不是Skip，故不剪枝目录子树，只跳当前entry，子文件可再创建父目录。
- walker只在callback开头检查cancel；阻塞send与worker复制无token，已入队项会继续完成；最后join所有worker但忽略panic，walk错误/send错误均忽略。整体可Ok带部分stats，即使worker失败/取消；issue仅部分filesystem失败，不能以issues空证明全量成功。worker panic而原channels仍持Receiver时，特定shard发送还可能阻塞，不保证失败及时终止。
- worker按rel_path直接join，无containment检查；源路径与type之间可变，非dir/nonlink统归File（含其他类型）。父目录在create成功前就放缓存，失败后同worker后续不重试mkdir；AlreadyExists视成功未确认是目录。目录计数是成功处理entry次数不一定新创建数，父目录隐式创建未计。
- regular复制后metadata读取失败仍记成功/计数但无index metadata；symlink保持link target。destination/source父symlink没有nofollow防护，当前复制不是安全隔离或一致性snapshot。
- 确认前述Clean+Ignored Copy风险：后阶段skip仅先前实际成功copied集合，不是所有unignored；Clean阶段跳过dirty文件会在第二次respect_gitignore=false遍历中再次复制，因此不能规范成“只复制ignored”且保持clean。copy_ignored_only则使用单独unignored集合，行为不同。
- engine六测试覆盖简单数量、skip、ignored-only、预cancel0、正常token和真实symlink，未注入中途取消、worker失败/IO partial、目录glob不剪枝或Clean+Copy组合。

copy/gitdir.rs 569行仍待阅读。

## gitdir与Git discovery完整阅读

完成copy/gitdir.rs569行、git/mod.rs及git/discovery.rs全部。

- gitdir要求source_git.is_dir，linked .git文件直接拒绝，不自动解析common dir；若source为linked worktree，standalone copy分支会失败（Linux快照另论）。is_dir可跟随symlink，不验证完全独立的对象库。
- 全树先递归创建目的目录并收集全部文件/symlink work_items，再复制；并非注释“仅objects并行、其他串行”。>=64项且worker>1时按连续chunk划分所有项，不是round-robin；worker数min(CPU,items)，无普通engine的8/32上限，也无cancel token。
- 按spawn顺序join全部线程并返回第一个错误，线程panic转Err。read_dir遍历本身未排序，因此“第一个错误确定”只限某次已生成work_items顺序，不保证跨运行相同。错误时已建目录及已复制文件保留，清理由上层负责。
- 任意depth名字以.lock结尾整项跳过（包括目录），top transient清单还含fsmonitor--daemon及ipc，与snapshot cleanup清单并不完全相同；非regular/nonlink条目按类型跳过。保留hooks、config、objects/info/alternates、marker等未做重写，不能保证全部仓库绝对路径引用去除。
- Unix symlink原样新建，不替换已有目标；非Unix以clone_file复制目标内容，却仍记symlinks_copied，非原样link语义。后续stat类型可变化，无一致性锁/no-follow保护。
- 测试覆盖skip各类、真实Unix socket、hook和marker精确内容、linked .git拒绝；128文件+显式4workers+目标obj0目录确保并行错误路径报对应条目，不依赖CPU或权限，是有效错误传播证据。基础copy只检查exists/count，不验证Git对象完整性或源删除后独立性。
- find_git_dir目前allow(dead_code)且仅测试，不应写作生产入口；find_worktree_git_dir只检查输入根下.git，文件严格从字节0匹配gitdir: 前缀然后trim路径，相对join/canonical失败回原路径，无路径归属验证（与api函数先trim整个文本不同）。
- find_worktree_root返回gix.workdir并拒绝bare，保留linked工作目录自身而非主仓库；get_head_commit必须peel commit，unborn报错。测试只40位hex（SHA1夹具），不能将公有返回类型限定成永远SHA1；relative gitdir测试准确核对canonical结果。

## Git index/status/worktree全部阅读

完成index307、status106、worktree399行。

- copy_git_index通过两端.git指针解析，source index不存在返回false不动dest；存在则先best-effort删dest再clone，后link shared indexes，无事务回滚。source=dest等同路径无保护。sharedindex从common dir与own dir所有前缀项扫描，不验证内容hash/类型，read_dir错误跳过；Unix直接symlink源路径（相对源路径不会额外absolute），非Unixclone。目标exists就跳过，dangling link可能造成create失败，不能视为稳健全量split-index恢复。
- resolve_common_dir读commondir trim并join，canonical失败回原；无内容路径验证。update_index_stats对空metadata、缺失/metadata错误/0字节index返回Ok；其他index使用固定Sha1解析，不按仓库objectFormat。路径lossy UTF8查entry，找不到忽略；Unixmtime/ctime/size/dev/ino窄转u32，nonUnix以mtime代ctime，不改entry mode。即使没有任何entry命中仍write，stat来自copy时刻而非写前重查。
- get_modified_files仅index-vs-worktree，不提供HEAD-vs-index staged变化；缺失/metadata失败/空index直接空结果，会漏未跟踪项。gix status由grow-gix-status预算线程，iterator错误传播；Modification Removed计deleted，其余modified，DirectoryContents计untracked，Rewrite计modified且只存dirwalk目标path。BString.to_string到PathBuf可能lossy；计数按事件而DashSet去重，不能保证计数和集合size相等。index/status没有内置测试模块。
- worktree add调用detached no-checkout，无deadline/cancel/显式--分隔；错误stdout不采只stderr上下文。公有stale清理限定exact或prefix，不用全局prune；exact目标exists早退，git common-dir查询失败/读目录失败返回0。
- registration必须is_dir且无locked存在；backlink按registration根解析相对，取parent但不验证末尾.git；recorded.exists则保留，missing经最深已存在祖先canonical加缺失tail进行匹配。无owner标记验证，prefix范围由调用方负责。错误多忽略或warn，只有remove_dir_all成功计数；没有锁或删除前二次检查，存在性权限错误可能作missing。
- 7项worktree测试真实git命令check成功，验证只删目标、保留foreign/locked/live/无backlink、相对backlink模拟、symlink父路径规范化；非Unix最后一个测试仅建夹具无核心断言。测试没有require_git宏，此处Git缺失会明确失败而非跳过。

Git剩checkout.rs1272行待读。

## checkout.rs 1–850行

- git_command使用PATH git、detach、stdin null、pager env、四项prompt/LFS/SSH覆盖及--no-optional-locks；未清除其他GIT_*环境/仓库config，无进程timeout或cancel。reset/checkout参数未显式--隔离，clean -fd/-fdx有一次force，不能宣称清所有嵌套Git仓库。
- tracked/staged diff-index退出0false，1或其他退出true；spawn错误仍Err。at_ref两次独立rev-parse commit peel，任何失败false，比较时ref可并发变化。capture返回lossy trim stdout，不保留原始bytes/末尾空白。
- snapshot仅覆盖autocrlf/longpaths/symlinks/quotepath/fsmonitor五配置，不阻断attributes clean filters、其他全局exclude或环境影响，注释“独立于用户全部config”太强。scratch名称pid+nanos+counter，未预先create_new占位，无安全独占保证；drop删index及display字符串拼.lock，错误忽略。
- snapshot用scratch read-tree HEAD → add -A → write-tree，再固定作者/提交者identity commit-tree parent HEAD及无old-value update-ref。工作目录快照内容不是保留两阶段staging语义；真实index不用于seed。HEAD在seed与commit-parent之间未固定，可并发移动。ref完全限定只是文档要求，入口不校验；失败可能留下Git对象，无业务事务清理。
- transfer无条件fetch --no-tags force refspec，即使linked共享存储也执行fetch，不是实际no-op；随后只验证目标ref能peel commit，不核对源快照SHA相等或fsync。所有路径lossy；caller需在transfer成功后自行决定删除。
- rehydrate尝试snapshot首parent并cat-file存在（失败统一fallback snapshot）；此前未验证snapshot整体有效就可能删除已存在dest。remove错误忽略后raw remove_dir_all重试，再限定stale注册清理；不验证dest是本功能所有。add后read-tree --reset -u snapshot保留base HEAD但把snapshot写入index，恢复差异通常为已暂存，不恢复原XY状态。
- populate失败best-effort remove与stale清理；add失败本身无guard，无token/delegate。metadata时best-effort登记Subagent/linked/ref HEAD/session，report commit是恢复HEAD（base或snapshot），复制统计全0，不代表无文件落盘。
- 测试读至850：真实reset、tracked/staged区分、at_ref变化，snapshot跟踪/非忽略新增、忽略排除、HEAD tracked后ignore仍捕获、clean tree SHA相等、两次ref覆盖。大多snapshot内容通过trim helper比较，只覆盖无尾部空白文本；精确tree相等是有效干净树证据。剩850之后恢复及迁移测试待读。

## checkout.rs 全部1272行阅读完成

- snapshot survives linked deletion实际删除工作树后从主仓库核对ref SHA和内容，是共享对象存储保留证据。does_not_mutate_real_index实际比较trim后的porcelain，不是注释声称index字节相同；不证明stat cache等索引二进制不变。
- CRLF capture测试只查存在任一CRLF；roundtrip测试则精确比较CRLF与LF完整bytes，并验证unchanged/edited/deleted/untracked内容以及HEAD=base，属于有效内容恢复证据。仍未测attributes filters或原XY staging恢复。
- base missing用parentless commit模拟，不是存在parent指针但object缺失，故未覆盖cat-file过滤分支。重复rehydrate实际成功目录再次删除重建；stale注册回归同时确认隐藏foreign worktree记录存留。
- standalone transfer测试先证明source无ref，再fetch核对SHA，删除standalone后仍核对SHA并rehydrate内容，有跨存储生命周期证据；无失败迁移/并发ref变化/fsync durability覆盖。
- metadata恢复用GrowHomeFixture及唯一dest、路径过滤查Subagent/head/session，明确记录正确。其他带默认DB写入的恢复测试仍依赖后续进程级GROW_HOME隔离。

Git目录六文件全部审阅完成。下一步sync.rs及auto_gc/db/discovery、Linux模块与CLI/bench/integration。

## sync.rs 1–790行（生产逻辑完成）

- SourceDirtyState私有Bytes raw仅缓存porcelain v2 -z --untracked-files=all，clone共享，不带source身份/HEAD/时间版本；预计算后再读HEAD和实际文件，因此不保证同一时刻快照。WorktreeSync公开source/worktree路径，声明适用于linked但构造不检查对象库共享或路径归属。
- sync仅HEAD不同时hard reset；相同HEAD不会清理dest已有tracked修改/staging。git clean -fd可显式skip，默认不删ignored；因此“同步到source全部状态”依赖pool提供已清理dest，copy_dirty=false也不保证dest tracked clean。
- timed入口记录phase毫秒，sync_dirty_state单独调用不填总dirty_sync_ms；precomputed入口不记录timing均0，None与空状态置dirty_skipped，普通入口无dirty输出并不置该flag。阶段失败无回滚，前面reset/clean已落盘。
- parser按NUL记录但每块必须UTF8，非UTF8路径Err；普通/u共用处理，XY固定byte[2..4]无长度/char边界校验，格式畸形可panic。路径字段缺失fallback整行，无normal组件或root containment验证。生产数据来自Git而非任意wire，仍非通用安全解析器。
- sub字段S跳过全部子模块（rename额外消费orig），不更新子模块工作内容；不能把reset保证描述成完整submodule同步。u条目header注释/field处理未单独正确映射三stage，extract_index_mode_hash仍取普通4/7字段，无法保留unmerged三索引状态，需后续测试核对。
- type2不区分rename/copy score类型，无条件stage新文件与delete旧path；旧dest.exists才remove_file，删除错误忽略但deleted计数仍加，dangling link可能未移除；若是copy记录也会删orig。未校验orig必存在，缺chunk仍继续。
- ordinary按Y==D删除，否则copy；staged X==D单独cached删除，其他非点X提取mode/hash。copy source NotFound当成功跳过，但调用方仍copied+1；report不一定表示实际落盘文件数。目的路径父symlink/源类型变化无隔离，现有dest先best-effort unlink，无法原子替换或目录类型转换保证。
- staged真实使用update-index -z --index-info精确mode/hash，非注释git add/cacheinfo，可保留MM不同blob；writes发生等待输出前，write错误?会留下未wait child，无kill-on-drop，输入巨大/子进程输出管道背压无timeout。命令非零只warn不返回Err；rm --cached --ignore-unmatch --同样非零非fatal，argv无批大小上限；staged_entries只按命令整体成功计输入长度，不核对实际条目数。
- 生产逻辑已全部读取；测试仅到790行，完成same-head无dirty及head-moved开头，其余待审。此前发现的Clean+Copy等债务仍单独记录，运行时代码不动。

## sync.rs 全部 1913 行阅读完成

- 791–1460 的测试覆盖 HEAD 前进、dirty/untracked 内容、删除、分支切换和五次提交。rename 用例未检查 git mv 成功及旧路径消失，不能当成完整 staged rename 证据；copy_git_index 用例只检查 cached 文件名。staged deletion still-on-disk 只验证磁盘内容及复制计数。
- MM 用例同时精确核对磁盘 worktree-version 和 git show :file 的 staged-version（检查退出码），支持索引 blob 与磁盘内容分离保留。空格/Unicode 用例覆盖有效 UTF8，不覆盖非 UTF8。submodule 跳过是 synthetic porcelain 输入，非真实子模块集成。
- unmerged 路径提取测试的实际格式包含 m1/m2/m3/mW，能正确找到路径；此前生产注释漏 mW 不等于路径解析错误。但普通模式/hash 字段提取用于 u 条目仍不能恢复三 stage，保留该边界。
- 1461–1913：skip_clean staged 用例核对磁盘全文及 cached 名称，未核对 staged blob；sequential reuse 在两次同步间显式 reset --hard/clean，验证的是已清理 pool 前提。leftover 用例直接证明 skip_clean=true 保留未跟踪文件，false 删除。另覆盖 dirty deletion 与 branch jump 后的 dirty/untracked overlay。
- precomputed None、空状态均跳过 dirty；有内容时复制；同一 SourceDirtyState 可先后应用两个 linked worktree，测试精确比较结果。但未在 collect 与 apply 间修改 source，不能证明缓存具备内容快照或 HEAD 一致性。
- Unix symlink 测试通过 symlink_metadata/read_link 验证有效、悬空链接及 tracked target 更新；单独 copy 用例验证悬空目的链接替换成普通文件。均未运行，本节是测试代码审阅。

## auto_gc.rs 1–960 行

生产逻辑已读完（1–696），测试读至 960；剩余测试待读。首次合并读取输出发生截断，已按分段重读此范围，不将截断内容计为完整审阅。

- 默认 enabled、7 天 age、6 小时间隔，Manual=never；Linux 默认 orphan cleanup，rebuild 默认关闭且独立 24 小时间隔。layer 按 local > remote > defaults，kind map 逐键覆盖；env 仅支持单向 kill、强制 dry-run/rebuild 和全局 max age，不是所有布尔字段双向覆盖。
- env truthy 接受 1/true/yes/on/enabled，disabled 接受 0/false/no/off/disabled/空，trim 并忽略大小写；无效值不覆盖，max age 解析 u64。解析策略将 age 限在 1 小时–90 天，间隔 60 秒–7 天。公开 raw AutoGcOptions 不再 clamp；maybe_auto_gc 重套 kill/dry-run/rebuild，未重套 max-age env。
- auto GcOptions 恒 force=false，保护当前 cwd 的 canonical fallback；只有 Linux/macOS 或 dry-run 开启 age，其余平台清空全局 age 和 kind map。age_expiry_enabled 是配置路径开关，不证明本次运行扫描成功或发生删除。
- GC stamp 读取失败返回 Err，不可解析或未来时间视为到期；elapsed saturating_sub 与非负间隔比较。节流命中跳过包括 rebuild 的整轮。没有跨进程原子 claim/互斥，stamp 不是并发执行排他锁。
- rebuild 可选且 dry-run 跳过；独立 stamp 读取失败仅跳过 rebuild。home 从 resolve_grow_home 取得，未从传入 db 推导；失败仅 warn，GC 继续。成功重建的修改立即存在，无跨阶段回滚。
- source repo 集合在 rebuild 后、dead GC 前读取，包含 dead，排除字面 unknown，排序去重；list 错误回空。GC 成功后 Linux orphan cleaners 顺序执行（仅非 dry-run 且开启），再对 grow_home 范围做 stale 注册清理；后者每个整轮执行，不受 rebuild 独立节流限制。
- GC Err 保留可能已重建的数据但不写两种 stamp；remove_failed 或 orphan errors 仅记录，仍可写成功 stamp。rebuild stamp 与 GC stamp 分别 best-effort 写，无事务，任一失败 report 对应 false。dry-run 仍可写 GC stamp，从而节流之后的真实回收。
- 测试前段确认平台开关、force=false、kind 默认、真实 own-CWD 可见性以及 dry-run age 配置路径；非扫描平台 own-CWD 测试提前返回。环境清理用模块锁并删除变量而不恢复原值，整包测试须进程级隔离；真实过期删除测试从 934 行附近开始，当前只读到 fixture 创建，后续断言待核对。

## auto_gc.rs 全部阅读完成

- 剩余测试分段读完至文件末尾。真实删除用例核对普通过期目录消失、当前 creator PID 目录保留和 Manual 默认保留；使用自定义 DB 而未在这些用例隔离 GROW_HOME，因此没有证明删除后从同一 DB 注销。CWD 保护用例使用 dry-run，验证不计入 would-expire，并非实际删除路径下的 CWD 保护端到端测试。
- regular-file 夹具在扫描平台要求 remove_failed>=1，且仍 stamp；其他平台只测 dead-only 路径。drop worktrees/meta 表验证错误返回；SQLite INSERT trigger 验证 stamp 写失败仍 Ran。这些是可定位故障注入，而非仅命名推断。
- dry-run orphan 测试及 Linux/non-Linux 对应分支检查 report 的 Some/None，Linux 非 dry-run 用例未造 orphan，不能证明实际 snapshot 删除。env kill/dry-run、numeric clamps、kind map、local/remote 优先级均有表格或明确断言；Manual 的 never 只保护年龄过期，missing Manual dead 记录仍注销。
- rebuild fixture 仅创建空 .git 目录，可证明 discovery 注册分类，不证明合法 Git 历史。真实 Git stale 注册测试明确检查命令成功、磁盘注册数量减少；dry-run/关闭开关/foreign grow-home 外注册保持不变，sole dead row 被注销后仍清理其 source repo 注册。
- rebuild INSERT 失败继续 dead GC；后续 sweep UPDATE 失败则两 stamp 不写，同时断言前面新登记记录仍存在，覆盖部分落盘语义。rebuild meta 读取失败仅通过 classify helper 注入，非真实 DB 分键故障。stamp INSERT 失败的 rebuild_stamped 断言带 if report.rebuild.is_some，并非强制证明 rebuild 执行。
- same-pass 新登记不被 age=0 删除的夹具没有主动设旧 mtime，且按秒取时间，未覆盖执行跨秒或慢速发现；不能将此用例扩大为绝对同轮保护保证。symlink escape 用例的目标仍在 grow_home 下，但不在 worktrees/repo 的扫描结构中，证明该符号链接被拒绝，不是 grow_home 外逃逸的独立用例。

## db 模块全部阅读（mod 461 / queries 239 / schema 35 / tests 690）

- metadata feature 门控本模块；禁用时公有 DB 类型不存在，模块注释“所有 DB 操作 no-op”不能当成 API 保证。WorktreeKind 六种 lowercase，lossy 未知退 Manual，exact 不接受大小写，config parser trim+ASCII lowercase；未知 status 退 Dead。serde enum 本身仍拒绝未知值。
- open(grow_home) → worktrees.db；open_at 先 mkdir parent，再通过 sqlite-journal 选模式/有效路径，network TRUNCATE 使用 per-host sibling，不迁移 legacy rows。journal Busy/Locked 重试预算约 10 秒，每次 busy timeout 最多 1 秒并 sleep 20ms；其他错误立即返回。预算检查在调用前，非硬实时 deadline。成功后恢复 5 秒 busy timeout 并建 schema；in-memory 仅建 schema。
- schema v1 的 id primary key/path unique，无 kind/status/JSON CHECK；初始化 CREATE IF NOT EXISTS 后读取 schema_version 的任意 query 错误都当缺失，低版本或无效值只写版本号，没有列迁移；高版本不拒绝。schema 初始化与版本写入没有显式事务。
- register 是 INSERT OR REPLACE 全记录替换，id 或 path 冲突均可能删除旧记录，不是只更新给定字段；路径转 lossy 字符串且不 canonical、不验磁盘存在。row 读取将非法 metadata 丢为 None，creator_pid i64 as u32 可截断；其他列类型错误传播。
- get 含正斜杠才作为路径 canonical fallback 查询，除此先 ID 再 label；Windows 反斜杠绝对路径不会走 path 分支，包含 / 的 ID/label 也不会按 ID/label 查。显式 get_by_id 不 fallback。label 查 valid JSON 的 $.label，按 created_at 降序取第一，允许 dead 行、重复标签和时间并列不确定顺序。
- list 默认 SQL status=alive，显式 status=Dead 仍需 include_dead=true，否则两个条件相交为空；repo_name/source_repo/kind/status 参数化 exact 相交，source 不 canonical，created_at 降序无稳定并列键、无分页。get/unregister_by_path 规范化行为不对称（后者仅原路径 lossy exact）。
- mark_dead/touch 只按 id，无 alive 前置条件；touch 不复活记录。stats 用两个独立 count 查询，无读取事务，dead=total-alive saturating，包含未知 status；db_file_bytes 是 page_count*page_size 的逻辑估计，不含 WAL/SHM 且非实际 stat 文件大小。
- sweep_dead 先收集 alive 全表，row 解码错误忽略，Path.exists 的权限错误/悬空链接都可能当缺失；逐条 UPDATE 只按 id 无路径/状态再次核对，无事务，失败前修改保留；marked 按循环次数加一，不检查 UPDATE affected。因此并发 register 同 ID 可在旧路径判断后被标 Dead。
- id_from_path 为去 worktree- 前缀的 basename + raw full-path 16 hex hash，不 canonical、不保证无碰撞。repo_name 无 basename 退 repo；时间早于 epoch 退0。GROW_HOME 接受空值/相对路径且不 canonical，非Unicode env 当不可用；fallback 读取 HOME 再 canonical fallback/.grow，与 config home resolver 的环境读取方法刻意不同。
- GrowHomeFixture 锁保护 setters，预热 DB 错误被忽略，保存/恢复 OsString，不能保护不取锁的 default DB reader。测试多数 in-memory，覆盖 CRUD/排序/JSON/label优先与重复标签/两 repo 同 basename 共存/删除一条不伤另一条。没有覆盖状态过滤矛盾、原始损坏 PID、跨 id/path 双冲突等边界。
- 并发 open 测试启动 16 thread，没有 barrier 强制同时转换；journal 本地 WAL 测试在 env override 存在时直接返回。network 测试在本地磁盘强制 seam，核对新库无 legacy 行以及 drop 后无 WAL/SHM，未在 NFS 执行。contention 测试实际持有 WAL read transaction，要求错误包含 database busy after、耗时<20s，释放后成功切 TRUNCATE；不验证严格10秒上限或等待下限。

以上均为代码/测试审阅，尚未执行 fast-worktree 动态测试。下一步 discovery、Linux mount/btrfs/overlay、CLI/bench/integration。

## discovery / mount_info / util 完整阅读；btrfs detect 至 520 行

- discovery 399 行全部读完。只扫描 grow_home/worktrees 和 worktree_pool 固定两层，分别 Session/Pool，非递归辨识真实工作树；目录无 .git 也进入 found，creation_mode=unknown。点前缀和 ready/claimed/claiming 后缀跳过，outer 非目录不计 skipped，inner 非目录计 skipped，read_dir/entry 错误静默略过；is_dir 跟随符号链接。
- linked source_repo 仅 trim 后严格 gitdir: 空格前缀、原文路径取三层 parent，不按工作树解析相对 gitdir，不验证 .git/worktrees 布局，也不读 commondir。standalone source_repo 就是自身；缺失退 unknown。into_record 使用 filesystem created 时间（不是 mtime）失败取 now，canonical fallback 生成 id，其他 session/PID/HEAD/metadata 都为空。
- rebuild 对 canonical fallback path 与 canonical fallback 两个 managed roots 做组件前缀判断；root 本身若是指向外部的 symlink 会被视为管理根。公开 path_under_managed_worktree_roots 不 canonical 输入 path；canonical 失败无强制拒绝，也没有删除/登记前防 TOCTOU。
- 重建先查 id 或 path（包括 dead）已有则跳过，不复活、不刷新 touch；新项 last_accessed_at 使用循环前一次 now，随后 into_record 又 canonical，可能与检查时路径不同。逐条写，无事务，后项失败前项保留；discovered 包含因边界拒绝的项，不保证等于 registered+already_tracked。
- discovery 测试用 fake .git，验证固定分类/marker/重复/不同 repo 同 basename/报告 serde/touch 存在。独立 symlink escape 夹具目标确实在 grow_home 外，断言发现1登记0；这是比 auto_gc 同名夹具更强的范围证据。没有相对 gitdir、unknown 目录、已有 dead、不完整扫描错误测试。
- mount_info 531 行全部读完。parse_mountinfo 每次读 self 文件，parse_line 忽略畸形行；不是跨模块缓存。保存 root/source 原始字符串，只 mount_point 做解码；options 以逗号首个 key=value，overlay 必需 lower/upper/work 且多 lower 仅取第一（先按冒号分割再解码）。
- find_mount_for_path/find_overlay_mount 使用 lossy 字符串前缀+下一字节斜杠，根挂载 / 对 /foo 下一字节为 f 因此不会匹配，测试仅覆盖 /workspace/repo，未覆盖根挂载回退；未 canonical，overlay 查找仅在 overlay 候选中取最长，不考虑更深非-overlay mount 遮挡。
- unescape_mountinfo 按每个原始 UTF8 byte 转 char，非 ASCII 文本会改变；转义检查允许 0–9 非严格八进制，u8 乘加对较大数字 debug 可溢出 panic，不能称通用字节保真 parser。合法 ASCII 空格/反斜杠/转义冒号用例有覆盖，非UTF8 read_to_string 本身失败。
- overlay_upperdirs_all_namespaces 遍历可读 /proc 数字 PID mountinfo，错误跳过且不报告未知，非实际全 namespace 完备性保证；提取 upper 解码但不 canonical，无 namespace 去重优化。调用方不得从空集合推导所有挂载均不存在。
- namespace 分类只是 self 与当前可见 PID1 的 ns link 相等/不同/读取失败，Host 不证明物理主机 namespace；Unknown 仅 log 一次且日志总归因 pid1，实际 self 失败也可能触发。is_fuse_mount 要求挂载点 exact，接受 fuse/fuse.*/fuseblk*，不检查子路径。测试为 synthetic 字符串/helper 比较；current 一致性测试只连续两次相等，不验证真实 namespace 拓扑。
- util 7 行使用 epoch 秒字符串，早于 epoch 退0，无单独测试。
- btrfs/mod 24 行读完，detect 1–520：statfs 错误传播；findmnt 非零/启动失败退 None，OsStr 参数保留输入路径 bytes，但 stdout lossy+空白拆分不解码，bind 判断使用 options.contains(bind)/source方括号/非dev绝对路径启发式。后续解析 /proc 失败可 Err，故注释“检测失败均 None”不完整。
- btrfs detect 没复用 mount_info parser，直接逐行空白拆分，不做转义解码。resolve_bind_mount_source 只 exact target，不找包含路径；同设备先第一个 root=/ mount，再第一个 subvol prefix mount，无最长匹配/验证路径存在。subvol helper 根 / 前缀同样可能因去掉斜杠后的 boundary 判断不匹配子路径，不过正常调用先尝试 root-mount 分支。
- get_btrfs_mount_point canonical fallback 后按 Path.starts_with 取最长 BTRFS entry，这里组件匹配支持 /，区别于共享 parser；只看 BTRFS 条目不看其他 mount 遮挡。is_btrfs_subvolume 用 PATH btrfs subvolume show 退出成功作判断，失败无细分，stdin null/detach，无deadline；确定 subvolume 后 mount-point 查询错误被 .ok().flatten 丢弃，报告仍可 Some。非BTRFS fallback 检查解析出的 source；已经BTRFS的bind分支不再次检查source。测试从 440 附近开始，读至根 statfs 测试开头；余521–728待审。

## BTRFS 三文件全部阅读完成

- detect 剩余521–728测试：root/tmp只断言调用Ok，不断言文件系统类型；真实BTRFS/subvolume用例先用被测检测器寻找环境，失败即提前return。bind可选测试也先以 get_bind_mount_info 判定，不识别会skip，故无法独立捕获该检测器false negative。subvol解析helper有exact/nested/partial-name/不同设备/非BTRFS模拟用例，无真实挂载建立。
- snapshot1048行全部读完。create_snapshot/delete_snapshot直接执行PATH btrfs subvolume命令，保留OsStr参数，detach/stdin null，失败携stderr；无--分隔、超时、取消、sync等待或内部ownership验证。create_snapshot注释的dest不应存在/parent存在并未由入口强制预检，实际依赖CLI语义，不能把该入口当路径隔离层。
- create_snapshot_with_symlink无bind时直接dest创建并返回，不建父目录、不写metadata；有bind必须mount point，hash full raw dest命名。mount==subvolume_root为词法相等时选.grow-snapshots，否则worktrees；非UTF8 basename退snapshot。
- 已有snapshot检查先用exists：有效symlink被unlink，悬空symlink不进入清理分支。真实候选需safe guard，再metadata Matches/Absent允许删除，Mismatch拒绝；Matches不判断目标工作树是否仍存活，调用方负责重建授权。删除旧snapshot后未删除旧meta，create失败可能遗留旧meta。
- 新snapshot后只尝试remove_dir空.grow-snapshots占位，非空/权限失败debug保留；expose失败best-effort delete新snapshot并返回原symlink错误，reclaim错误完全丢弃，不写恢复metadata。成功expose后写meta失败warn仍返回成功，会影响后续orphan发现。
- create_worktree_symlink mkdir缺失parent，lstat成功时现有symlink无条件unlink，其他条目尝试remove_dir，只允许空目录，普通文件/非空目录失败。lstat其他错误不单独传播，继续symlink；检查与操作非原子，target原路径直接写链接，相对target由dest parent解释，无canonical。
- safe delete guard拒绝ParentDir、候选本身symlink、无filename、不可canonical parent；parent最终名必须worktrees/.grow-snapshots且grandparent匹配可见BTRFS mount。并不检查候选存在/是subvolume/名字hash/会话所有权；测试明确接受不存在候选。mount读取失败空列表拒绝，但仅靠目录名+mount锚点不是所有权证明，metadata检查是另一个步骤；没有fd绑定或并发替换保护。
- metadata sibling要求UTF8 basename，serde字段type/snapshot_path/mount_target/created_at；直接std::fs::write，无原子rename/fsync/防symlink覆盖。snapshot_meta_state读取缺失（含dangling meta）为Absent，其他读取/解析错Mismatch；Matches只比mount_target词法相等，不核对type及snapshot_path。remove metadata忽略错误。
- 测试覆盖metadata roundtrip、路径hash形状/不同repo区分、Absent/Matches/Mismatch/坏JSON、真实symlink读内容/替换/空目录/拒绝非空保留内容/parent file失败。safe guard全用临时目录注入“mount”，不是真实btrfs权限删除。expose测试仅注入成功reclaim closure，未验证reclaim失败后的磁盘恢复或真实subvolume删除。
- 真实snapshot测试有固定/tmp目的地或BTRFS_TEST_PATH拼接目的地，测试前会btrfs delete和remove_dir_all已有路径；不能在未隔离的Linux用户环境直接运行。真实创建Err仅打印并使测试通过，成功也只断言exists，未校验CoW隔离/内容/Git有效性，cleanup错误忽略。当前macOS不会编译这些Linux模块；未来测试结果必须单列平台与执行/skip边界。
- 修正此前util“无单独测试”的范围：util.rs内部确无测试，但snapshot::tests::test_unix_timestamp_string覆盖后缀、u64解析和秒值下限。

下一步overlay三文件、两个bin与integration；本包仍pending。

## overlay 三文件与 CLI 完整阅读，bench 至285行

- overlay mod15/detect190/snapshot762全部读完。检测依赖共享mount parser：包含overlay、第一lower exact FUSE mount、upper statfs BTRFS、三options可解析；不检查upper父目录为subvolume、不检查upper名称为upper、不验证work与upper同父。parse/statfs全部错误降为None，不限EIO/ENOTCONN。overlay_root取upper.parent，后续创建却固定snapshot/upper，不适用于任意upper basename。
- detect测试主要negative/helper和Debug；fields只要求Ok，没有断言第三阶段失败或Some。/tmp无overlay是假设，非隔离fixture。功能文档不能据此声称真实FUSE+BTRFS检测已跑通。
- 创建以dest UTF8 basename（否则overlay-wt）作为overlay_root/worktrees目录名，无全路径hash，跨repo同名会共用backing路径。已有root尝试删除错误忽略，无metadata ownership/live mount/safe BTRFS guard；old work目录直接remove_dir_all。snapshot整个overlay_root，固定root/upper，create_dir新overlay-work，不检查source中已有同名目录。
- snapshot成功后work_dir创建、metadata写入、dest mkdir失败均直接?返回，无本函数补偿；只有mount失败会尝试delete snapshot、remove work/base（错误忽略），dest可能保留。delegate仅负责mount，不代替snapshot创建；直接路径调用libc mount flags0/index=on，target bytes保留，lower/upper/work用display拼option不escape逗号/冒号且lossy，无复制源mount额外options。
- metadata位于base，直接write不原子/fsync，created_at实际epoch字符串而非struct注释ISO8601；type读取不校验。创建dest mkdir允许已有非空目录被mount遮盖，无空目录/路径归属检查。
- remove先delegate或MNT_DETACH unmount，失败仅warn，再查可读namespace upper集合；root basename决定upper推导，集合匹配词法非canonical。可见active拒绝继续，但扫描不完备、lazy detach也不证明无仍持有的引用。然后best-effort remove target、delete snapshot、remove work/meta/parent；snapshot删除失败仍清metadata，最终Err，不能保证后续metadata恢复可发现。成功report unmounted_overlay=true不证明unmount成功。
- mountinfo移除入口exact overlay target就采用其upper/work，无本项目ownership判定。extract_option值这里没有反转义，区别于detect；parent名root决定新布局。metadata恢复只扫/local/repo-fuse-*/worktrees/*，与模块示例/var/lib不同，匹配lossy target后直接信任snapshot_root/work_dir，无type/containment检查。
- orphan同样固定/local前缀，read_dir errors略过，跟随目录symlink，无safe guard；活动upper集合只取一次。新root存在否则旧upper，metadata只用于额外umount/remove target，不校验它与扫描snapshot对应。snapshot删除失败仍清work/meta，report removed仍+1，不代表整项实际回收；可能保留无metadata新root，下次按布局仍可重试，但丢失原target恢复信息。
- snapshot测试主要JSON/write位置、no-overlay和/no-local环境假设。cleanup_no_local实际调用全局cleaner，没有构造隔离/local；不能在有真实repo-fuse数据的Linux宿主直接作为无副作用单测运行。metadata_scan_finds_matching_target仅serde断言，未调用scanner；无真实mount/unmount/跨namespace/故障补偿测试覆盖。
- CLI185行全部读完，仅create子命令。default HEAD，dirty=false选择CleanAll（区别builder默认Preserve），ignored=false跳过且skip参数此时不用；两个parallelism默认0，channel_buffer固定1024，standalone布尔。无kind/session metadata设置、取消token/delegate入口。Result错误返回失败，copy issues仅打印warnings仍成功。
- CLI展示commit[..12]假设长hash；files_copied==0直接标snapshot，空repo/仅symlink/复制失败也可能0，不能把输出当实际strategy证据。Mode按请求standalone而非执行结果显示，Linux snapshot路径可能与linked标签不符。无本地CLI测试模块。
- pool_perf_bench808行读至285：参数source默认当前目录、iterations3、parallelism0、copy_dirty/ab/json/verbose。phase_create实际GitCheckout builder；其他git命令没使用库统一git_command/detach/env覆盖。warm第一遍非零只写detail，后续status/diff多数不查退出码；release reset+clean -fdx报告布尔但不返回命令非零错误。性能数字本身不能证明cache填充或完整生命周期行为正确；剩余286–808待读。

## 全包源码阅读收尾：bench808 / integration938 全部读完

- bench A/B 虽参数注释concurrently，实际上所有create/warm/sync/use/release/cleanup串行；两个模式均skip_clean=true，每轮新建工作树而非跨轮复用。simulate use只是git diff，不制造修改；copy_dirty读取真实源状态。phase_cleanup调用库remove_worktree，不是头部注释git worktree remove。
- bench canonical source失败Err，tracked计数错误退0；临时目录唯一，但任何中途?失败只有TempDir析构磁盘清理，没有确保Git注册注销的guard。运行前直接修改source core.fsmonitor/core.untrackedCache=true，错误忽略且不恢复，iterations=0也修改。没有运行本基准，避免修改本项目Git配置。
- summary按首轮phase名遍历后续数据取均值，total含日志等额外开销；max以严格大于比较，空轮返回0/(none)，不是统计置信区间。文本source按55字节切片，非UTF8字符边界可panic。JSON手工Rust Debug字符串输出，特殊字符可能产生非JSON转义；tracing默认writer也可能污染stdout，json模式仍有phase stderr进度。
- overlay integration整个文件Linux门控；helper自己解析live mountinfo、取首个满足条件stack，不要求工作目录或专用环境，缺失返回跳过，未校验CAP_SYS_ADMIN/源Git/subvolume root。字节解码逻辑复制了生产Unicode边界。unique_name是pid+毫秒模一百万，并非严格唯一；所有builder dest basename固定mnt，导致当前overlay backing统一指向worktrees/mnt，与name前缀夹具不一致。
- detect测试未调用公有检测API，只断言helper结果；lower不存在的assert分支总true。raw snapshot测试对env.upper_dir做snapshot，当前production对upper.parent做snapshot；raw mount夹具work在snapshot外，与生产新布局same-subvolume要求不一致，不能代表当前布局集成。
- builder uses-overlay断言路径存在/复制0/commit非空/git rev-parse成功及remove flags，没有在创建时真正assert mountpoint。writes independence未预先确认源无固定测试文件，多个worktree测试同basename可能碰撞；cleanup多为忽略Result，assert panic时没有RAII teardown。
- git status首命令检查退出，第二只stdout包含新文件名；文件名固定且可能受source ignore/已有内容影响。remove/orphan/meta测试仍推导base=<name>,snapshot=upper，与实际basename=mnt/root/upper两处不同，必须标明夹具过时，不能宣称已覆盖当前成功路径。
- bulk cleanup测两层目录下mnt，要求removed>=2/errors0，后置只不存在或非mount，不能证明snapshot实际回收。orphan调用全局/local cleaner，可能清理其他测试/既有orphan；不是隔离集成测试。metadata survives手动unmount后忽略remove并raw清理旧layout，不保证当前root残留清掉。
- dedicated workdir最后用live mountinfo读取workdir末尾overlay-work，是比旧layout推导更直接的当前挂载证据；但仍需真实stack/权限，未设置delegate，不能证明rootless创建成功。没有执行这些Linux测试。

本包manifest及所有Rust文件已逐段读完，未发现其他非Rust资产（仅Cargo.toml）。下一步将本记录分解为正式需求/场景和来源映射，保留已知缺陷为事实边界及独立债务；在映射和验证完成前不标reviewed。磁盘复查余313MiB，尚不足以承担新增Cargo编译，因此动态测试仍待可用空间或现成产物证据，不把未执行视为通过。

## 功能映射完成

完整代码审阅及来源映射完成；reviewed表示完成该审阅任务，不表示动态测试通过。磁盘余量不足，尚未运行Cargo测试；Linux专属测试未执行。

## 功能与规范映射

- [Worktree feature availability](../specs/worktree-lifecycle/spec.md#requirement-worktree-feature-availability)：包 SHALL 提供同步创建、复制、同步、删除及Git快照接口；metadata开放DB/discovery/GC，Linux开放BTRFS与overlay，bench开放基准binary。
- [Worktree builder defaults and modes](../specs/worktree-lifecycle/spec.md#requirement-worktree-builder-defaults-and-modes)：WorktreeBuilder SHALL 默认HEAD、Linked、PreserveWorkingTree、Ignored Skip、自动并行度和256 channel buffer，支持Standalone/GitCheckout及CleanTracked/CleanAll配置。
- [Worktree plan parallelism](../specs/worktree-lifecycle/spec.md#requirement-worktree-plan-parallelism)：计划 SHALL 对普通和ignored复制分别选择显式并行度，否则使用CPU数。
- [Worktree disk space error context](../specs/worktree-lifecycle/spec.md#requirement-worktree-disk-space-error-context)：create SHALL 在错误链含StorageFull或选定ENOSPC英文消息时增加磁盘不足上下文并保留原错误。
- [Worktree privileged delegate boundary](../specs/worktree-lifecycle/spec.md#requirement-worktree-privileged-delegate-boundary)：BtrfsDelegate SHALL 提供Send加Sync创建删除委托及overlay mount/unmount扩展接口。
- [Worktree linked materialization](../specs/worktree-lifecycle/spec.md#requirement-worktree-linked-materialization)：Linked复制路径 SHALL 先Git detached no-checkout登记，再复制与finalize，Preserve使用源index及复制stat，Clean模式执行reset和相应clean。
- [Worktree standalone materialization](../specs/worktree-lifecycle/spec.md#requirement-worktree-standalone-materialization)：Standalone复制路径 SHALL 复制独立.git并并行复制文件，随后等待.git复制结果、finalize索引并处理非HEAD ref。
- [Worktree GitCheckout strategy](../specs/worktree-lifecycle/spec.md#requirement-worktree-gitcheckout-strategy)：GitCheckout SHALL 调用Git checkout并传入checkout.workers，创建真实linked工作树。
- [Worktree snapshot strategy selection](../specs/worktree-lifecycle/spec.md#requirement-worktree-snapshot-strategy-selection)：Linux Linked及Standalone SHALL 尝试overlay/BTRFS快照，失败可回退复制；GitCheckout使用Git路径。
- [Worktree snapshot Git cleanup](../specs/worktree-lifecycle/spec.md#requirement-worktree-snapshot-git-cleanup)：快照finalize SHALL 清理.git中选定瞬态文件及递归lock并按工作区模式清理，来源marker采用best-effort。
- [Worktree ignored only copy](../specs/worktree-lifecycle/spec.md#requirement-worktree-ignored-only-copy)：copy_ignored_only SHALL 先收集unignored集合，再关闭gitignore遍历剩余文件，使用skip patterns与独立worker配置。
- [Worktree copy traversal and filtering](../specs/worktree-lifecycle/spec.md#requirement-worktree-copy-traversal-and-filtering)：copy引擎 SHALL 使用ignore walker、显式.git排除、glob与skip_files过滤并分发文件、目录、符号链接。
- [Worktree copy sharding and backpressure](../specs/worktree-lifecycle/spec.md#requirement-worktree-copy-sharding-and-backpressure)：复制 SHALL 以父路径hash分片到有界worker队列，自动线程数在macOS与其他平台分别有上限。
- [Worktree copy report semantics](../specs/worktree-lifecycle/spec.md#requirement-worktree-copy-report-semantics)：worker SHALL 累计复制文件、处理目录及非致命issues，并尝试记录文件metadata供index stat更新。
- [Worktree CoW and symlink copying](../specs/worktree-lifecycle/spec.md#requirement-worktree-cow-and-symlink-copying)：文件复制 SHALL 优先reflink并回退普通复制，设置permissions；Unix符号链接保留link target。
- [Worktree git directory cloning](../specs/worktree-lifecycle/spec.md#requirement-worktree-git-directory-cloning)：独立.git复制 SHALL 枚举目录及文件，跳过lock、fsmonitor和选定瞬态文件，较大集合按块并行。
- [Worktree Git discovery and tracked count](../specs/worktree-lifecycle/spec.md#requirement-worktree-git-discovery-and-tracked-count)：Git辅助 SHALL 解析.git目录或gitdir指针、发现工作树根并读取HEAD；tracked计数读取gix index或HEAD。
- [Worktree Git index copying](../specs/worktree-lifecycle/spec.md#requirement-worktree-git-index-copying)：copy_git_index SHALL 复制源index并处理common/own目录下sharedindex文件；源index不存在返回false。
- [Worktree index stat refresh](../specs/worktree-lifecycle/spec.md#requirement-worktree-index-stat-refresh)：update_index_stats SHALL 按复制metadata更新命中条目的stat字段并写索引。
- [Worktree dirty status collection](../specs/worktree-lifecycle/spec.md#requirement-worktree-dirty-status-collection)：get_modified_files SHALL 收集index与工作区差异、未跟踪项及事件计数。
- [Worktree scoped stale registration cleanup](../specs/worktree-lifecycle/spec.md#requirement-worktree-scoped-stale-registration-cleanup)：stale注册清理 SHALL 限定exact目标或路径前缀，保留locked、live及不匹配注册，并尝试canonical缺失路径祖先。
- [Worktree Git subprocess environment](../specs/worktree-lifecycle/spec.md#requirement-worktree-git-subprocess-environment)：git_command SHALL 关闭交互stdin、detach并覆盖pager及选定认证/LFS环境，加入no-optional-locks。
- [Worktree scratch Git snapshots](../specs/worktree-lifecycle/spec.md#requirement-worktree-scratch-git-snapshots)：Git快照 SHALL 用临时index从HEAD执行add -A/write-tree/commit-tree并更新指定ref，保留真实index作为独立文件。
- [Worktree snapshot ref transfer](../specs/worktree-lifecycle/spec.md#requirement-worktree-snapshot-ref-transfer)：ref转移 SHALL 执行fetch no-tags force并验证目标ref可解析为commit。
- [Worktree snapshot rehydration](../specs/worktree-lifecycle/spec.md#requirement-worktree-snapshot-rehydration)：恢复 SHALL 尝试以snapshot父提交为HEAD，父不可用时回退snapshot，再read-tree reset-u恢复树。
- [Worktree sync HEAD and cleaning](../specs/worktree-lifecycle/spec.md#requirement-worktree-sync-head-and-cleaning)：WorktreeSync SHALL 仅在源与目的HEAD不同时hard reset，按skip_clean控制clean -fd，并可复制dirty状态。
- [Worktree precomputed dirty state](../specs/worktree-lifecycle/spec.md#requirement-worktree-precomputed-dirty-state)：SourceDirtyState SHALL 缓存porcelain v2 NUL状态Bytes并允许复用于多个同步目标。
- [Worktree dirty file and index replay](../specs/worktree-lifecycle/spec.md#requirement-worktree-dirty-file-and-index-replay)：同步 SHALL 处理普通、rename、untracked、删除与Unixsymlink，并用index-info写源staged mode/blob以保留MM区别。
- [Worktree sync parser and submodule limits](../specs/worktree-lifecycle/spec.md#requirement-worktree-sync-parser-and-submodule-limits)：dirty解析 SHALL 跳过submodule条目并按porcelain类型提取路径。
- [Worktree generic removal and batch scope](../specs/worktree-lifecycle/spec.md#requirement-worktree-generic-removal-and-batch-scope)：remove_worktree SHALL 先尝试Linux特定删除再普通symlink unlink或目录删除，成功后best-effort注销DB；batch扫描一至二层。
- [Worktree BTRFS removal and recovery](../specs/worktree-lifecycle/spec.md#requirement-worktree-btrfs-removal-and-recovery)：Linux删除 SHALL 处理snapshot symlink、direct/bind及metadata恢复，支持部分delegate回退。
- [Worktree BTRFS orphan reclamation](../specs/worktree-lifecycle/spec.md#requirement-worktree-btrfs-orphan-reclamation)：BTRFS orphan扫描 SHALL 使用已知mount存储目录及metadata识别目标，检查活跃mount或symlink引用并尝试回收。
- [Worktree GC age policy](../specs/worktree-lifecycle/spec.md#requirement-worktree-gc-age-policy)：GC SHALL 依次采用skip kind、per-kind期限、global期限，按max(created,last_accessed)计算年龄。
- [Worktree GC liveness protection](../specs/worktree-lifecycle/spec.md#requirement-worktree-gc-liveness-protection)：非force年龄回收 SHALL 检查保护路径、creator PID与可用进程CWD，并在删除前重查。
- [Worktree GC persistence boundaries](../specs/worktree-lifecycle/spec.md#requirement-worktree-gc-persistence-boundaries)：GC SHALL 对dead记录清理DB并对超期记录调用磁盘删除，dry-run只报告候选。
- [Worktree database schema and journal](../specs/worktree-lifecycle/spec.md#requirement-worktree-database-schema-and-journal)：metadata DB SHALL 存储worktree记录与meta，id主键及path唯一，按sqlite-journal选本地WAL或network per-host TRUNCATE。
- [Worktree database replacement and decoding](../specs/worktree-lifecycle/spec.md#requirement-worktree-database-replacement-and-decoding)：登记 SHALL INSERT OR REPLACE整条记录，路径保存为lossy文本；未知kind退Manual、未知status退Dead。
- [Worktree database lookup and filtering](../specs/worktree-lifecycle/spec.md#requirement-worktree-database-lookup-and-filtering)：get SHALL 将含/输入按canonical fallback路径查询，否则先ID再label；list提供状态、kind、repo及source exact组合过滤。
- [Worktree database maintenance](../specs/worktree-lifecycle/spec.md#requirement-worktree-database-maintenance)：DB SHALL 提供mark_dead、touch、stats和sweep_dead，touch不复活dead。
- [Worktree home and record identity](../specs/worktree-lifecycle/spec.md#requirement-worktree-home-and-record-identity)：默认DB SHALL 优先GROW_HOME，否则canonical fallback HOME/.grow；ID由basename与完整原始路径hash形成。
- [Worktree disk discovery](../specs/worktree-lifecycle/spec.md#requirement-worktree-disk-discovery)：discovery SHALL 扫描worktrees与worktree_pool两层，分类Session/Pool，跳过点前缀和ready/claimed/claiming marker。
- [Worktree database rebuild](../specs/worktree-lifecycle/spec.md#requirement-worktree-database-rebuild)：rebuild SHALL 检查canonical fallback目标在管理root内，按ID/path跳过已登记项，新项保存一次now为last_accessed。
- [Worktree auto GC configuration](../specs/worktree-lifecycle/spec.md#requirement-worktree-auto-gc-configuration)：auto GC SHALL 默认开启、7天age、6小时间隔、Manual never，rebuild默认关闭且24小时间隔；local覆盖remote再覆盖默认。
- [Worktree automatic GC sequencing](../specs/worktree-lifecycle/spec.md#requirement-worktree-automatic-gc-sequencing)：自动GC SHALL 先检查GC节流，再可选重建、收集source repo、GC、orphan及scoped prune，force恒false。
- [Worktree auto GC failure and stamps](../specs/worktree-lifecycle/spec.md#requirement-worktree-auto-gc-failure-and-stamps)：GC stamp读取失败 SHALL 返回Err，重建stamp读取失败只跳过重建；未来或不可解析stamp视到期。
- [Worktree mount parsing](../specs/worktree-lifecycle/spec.md#requirement-worktree-mount-parsing)：挂载工具 SHALL 解析mountinfo、匹配overlay/FUSE及取首lower层，并提供namespace状态与可读PID upper集合。
- [Worktree BTRFS detection](../specs/worktree-lifecycle/spec.md#requirement-worktree-btrfs-detection)：BTRFS检测 SHALL 使用statfs与btrfs subvolume show，再借findmnt和mountinfo解析bind来源及mount point。
- [Worktree BTRFS snapshot layout](../specs/worktree-lifecycle/spec.md#requirement-worktree-btrfs-snapshot-layout)：快照 SHALL 对无bind来源直接在dest创建；bind来源在mount下worktrees或.grow-snapshots以basename加完整dest hash创建并symlink暴露。
- [Worktree BTRFS metadata and exposure](../specs/worktree-lifecycle/spec.md#requirement-worktree-btrfs-metadata-and-exposure)：快照暴露 SHALL 允许替换symlink或空目录，拒绝非空目录；暴露失败尝试回收新snapshot。
- [Worktree BTRFS delete guard](../specs/worktree-lifecycle/spec.md#requirement-worktree-btrfs-delete-guard)：safe predicate SHALL 拒绝ParentDir与候选symlink，要求canonical父目录名为指定存储名且直属BTRFS mount。
- [Worktree overlay detection and layout](../specs/worktree-lifecycle/spec.md#requirement-worktree-overlay-detection-and-layout)：overlay SHALL 在FUSE lower与BTRFS upper条件下尝试snapshot整个upper父目录，并用root/upper及新overlay-work挂载。
- [Worktree overlay creation failures](../specs/worktree-lifecycle/spec.md#requirement-worktree-overlay-creation-failures)：overlay创建 SHALL 在mount前写base metadata，delegate可接管mount；直接mount使用index=on。
- [Worktree overlay removal and orphan scan](../specs/worktree-lifecycle/spec.md#requirement-worktree-overlay-removal-and-orphan-scan)：overlay移除 SHALL 尝试unmount并拒绝可见active upper，随后清snapshot/work/metadata；恢复与orphan扫描限定/local/repo-fuse前缀。
- [Worktree CLI behavior](../specs/worktree-lifecycle/spec.md#requirement-worktree-cli-behavior)：fast-worktree CLI SHALL 提供create、ref、dirty、ignored、skip、并行度与standalone参数；默认CleanAll及buffer1024。
- [Worktree pool benchmark scope](../specs/worktree-lifecycle/spec.md#requirement-worktree-pool-benchmark-scope)：pool-perf-bench SHALL 测量create/warm/sync/use/release/cleanup阶段并提供文本及JSON格式输出。
- [Worktree platform validation boundary](../specs/worktree-lifecycle/spec.md#requirement-worktree-platform-validation-boundary)：包验证记录 SHALL 区分实际执行、平台排除、环境跳过与仅代码审阅；Linux快照集成依赖真实受隔离stack。

## 边界

- metadata并未默认启用；default-bazel才启用metadata，不保证所有API跨平台可用。
- false不撤销此前Standalone设置；setter不提供完整输入合法性校验，策略执行仍受平台条件影响。
- 计划ignored阶段不继承它；copy_ignored_only入口却会在ignored未配置时继承普通值。
- 不保证有同一顶层磁盘不足标注；创建失败不能据此推导已完整回滚。
- 默认返回不支持；委托返回路径与原计划dest没有统一身份校验，能力由实现决定。
- 只排除前面成功复制的集合，先前dirty跳过项可能被重复制；Clean与Ignored Copy组合不保证最终仍干净。
- 部分提前返回发生在线程join/cleanup guard建立之前；HEAD报告可读取source而不是最终dest。
- 该策略不执行复制模式的这些处理，也不提供同样的失败清理guard。
- 快照包含源ignored内容，不执行普通ignored筛选或token取消检查；复制统计0不代表没有文件。
- 不会作为独立.git目录完整改写；CleanAll的Git clean -fd仍保留ignored，不等于删除所有未跟踪内容。
- 该入口仍执行复制；取消在复制后检测并返回错误，已落盘内容不回滚。
- skip不保证剪枝整个子树；遍历错误可被忽略，集合不等同Git index tracked语义。
- 取消主要停止walker，worker继续排空；send/join错误可能被忽略，保留receiver时worker panic可能造成发送阻塞。
- 目录缓存可能已写入；统计不证明全部内容成功落盘，issues没有独立数量上限。
- 删除旧目标是best-effort且非原子，无fsync/全部metadata保证；非Unix链接处理不提供Unix等价保证。
- 指针文件被拒绝；并行线程全部join并返回错误，可能保留部分dest，不改写alternates等外部路径。
- 相对路径基于工作树解析并canonical fallback；计数入口包含索引加载，不能视为恒定成本全树统计。
- 无统一原子替换/同路径保护；sharedindex未逐个验证hash，Unix链接使用原始源路径。
- 可直接成功或忽略条目；解析固定SHA1，路径lossy，不提供SHA256仓库与非UTF8完整保证。
- 此入口不提供完整staged状态；缺失/空索引可返回空，计数与去重集合大小不必相等。
- 可跳过错误且无原子再验证；调用方负责范围归属，不执行无限定全局prune。
- 仍可能影响命令；无统一deadline或取消，diff非零可视为有变化，at_ref解析失败返回false。
- 不保证同一时刻HEAD快照，也不保留原两阶段staging；update-ref无CAS，属性filter仍可参与。
- 仍会执行fetch；不比较转移前后源SHA，也不保证fsync持久化。
- 可能先删除目标；失败best-effort清理，恢复差异进入index，不还原原XY状态；metadata登记为Subagent。
- 不会先reset这些修改；skip_clean保留遗留untracked，普通clean也保留ignored，依赖调用方提供合适dest。
- apply仍读取当前内容，没有源身份/HEAD版本绑定；None或空预计算状态跳过dirty，预计算入口不提供普通timing值。
- 部分计数仍可增加；staged非零仅warn，不保证报告意味着全部状态已复制，也无阶段回滚。
- 不提供通用健壮解析保证；unmerged不还原三stage，type2可能按rename删除旧路径。
- 普通删除未统一验证ownership；lstat错误可按缺失成功，batch第二层不要求.git，注册清理错误可忽略。
- 某些路径仍清目标引用/返回成功；snapshot安全拒绝、实际引用清理与DB注销不是原子事务。
- 删除失败保留metadata，父目录缺失可跳过；坏metadata可被清理，报告计数不保证所有best-effort动作成功。
- never不被force覆盖；未来age按0处理，超期使用严格比较，dead清理独立于年龄保护。
- Failed阻止非force年龄删除，Unsupported与非Unix PID策略不等价于完整保护；扫描自己可见不证明全部进程可见。
- 磁盘remove默认DB注销可能不对应传入DB；重查非原子且可使用原记录path，不构成事务一致性保证。
- 约10秒预算重试后失败；schema v1初始化不提供完整升级迁移或高版本拒绝，network库不迁移legacy rows。
- 可能替换原记录；坏JSON读为None，raw PID整数窄转不额外校验，登记不验磁盘存在。
- label按created_at最新取一且不排除dead；后者过滤相交为空；并列排序不保证稳定。
- sweep可能把不可访问视为缺失且保留部分更新；stats容量为页数估计，不含WAL/SHM或事务快照保证。
- 不额外规范化或拒绝；hash不保证无碰撞，调用方的canonical时机决定记录身份。
- 仍可发现unknown模式目录；读取错误略过，不保证完整候选列表，linked source仅解析gitdir三层parent。
- 不复活或touch已有项；root本身可canonical到外部，逐项写入无事务，discovered包含拒绝项。
- 只支持相应单向覆盖，age clamp为1小时至90天、间隔60秒至7天；raw options不重复全部clamp。
- 节流跳过整轮，dry-run跳过重建/prune/orphan但仍可写GC stamp；非扫描平台真实年龄回收关闭。
- 前者不写两stamp且保留已重建记录；后者仍可stamp，stamp写失败仅report false，节流不是跨进程互斥。
- 根前缀匹配与逐字节解码存在边界；集合不证明全部namespace完备，Host只相对可见PID1，Unknown可能仍启用overlay。
- 可降为None或错误；解析按首个可用设备路径且不统一解码，subvol fallback不等于完整挂载拓扑解析。
- 相等才选隐藏目录；已有项依据安全路径及metadata三态处理，悬空snapshot symlink可能绕过exists清理分支。
- 回收错误丢弃，metadata失败warn仍可成功；metadata直接写无原子持久化，Matches只比mount_target。
- predicate可接受不存在候选，不证明subvolume或会话所有权；裸删除函数不自动执行guard。
- 检测不验证这些前提；backing仅按basename命名可共用路径，不提供BTRFS hash布局的同等隔离。
- 前者可直接返回留下snapshot；后者best-effort清理且dest可保留，路径option未统一escape。
- 失败仍可清metadata；扫描不完备且未统一ownership/containment校验，removed及unmounted标志不证明所有动作成功。
- issues打印warning仍成功；0计数显示snapshot不证明实际策略，mode展示采用请求值。
- A/B仍串行；启动会修改source Git性能配置且不恢复，部分Git非零不失败，JSON特殊字符串及stdout纯度无保证。
- 不证明Linux快照运行；现有overlay集成含旧布局和固定mnt basename夹具，BTRFS部分错误只打印，不能扩展为完整行为证明。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
