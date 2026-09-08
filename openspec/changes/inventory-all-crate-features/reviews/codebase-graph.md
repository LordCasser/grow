# codebase-graph 逐包审阅

状态pending。已完整读取Cargo.toml、lib.rs、types下mod/file_event/location/range及interner.rs（含6项测试）。尚未运行本包测试。剩languages、scope_graph、manager、index_manager、navigation、三个bin及integration。

## 入口与数据模型

- 默认features为空，三个bin为code-graph、bench_index、bench_file_listing；tree-sitter配Rust/TS/Python/Go/JS grammar，git2禁用默认feature。lib注释称mmap/zero-copy，尚未读存储实现，不能作为事实。root导出的FileEvent来自index_manager，而types::FileEvent是独立enum，后者Created/Modified需reparse，Deleted/Renamed不需reparse，rename主path为to；不能混同两套事件语义。
- FileMeta以size和mtime秒/纳秒判断stale，不hash内容；metadata失败为stale，mtime失败或epoch前回退0。SymbolOccurrence Arc字符串及1-based行，SymbolAlias仅alias/original，不携带路径解析保证。
- types::Location camelCase含filePath、line、column、range；new不验证绝对路径或坐标一致性，from_range加1，转0-based饱和减1。其注释将1-based称LSP兼容不准确，不能由注释推导LSP wire契约。root导出的Location来自navigation，尚待核对。
- Position保存0-based line/character及byteOffset；tree-sitter point.column直接复制，未转换UTF16。from_byte找首个line_end>byte，找不到回退line0，column沿byte算；不能保证EOF或缺尾哨兵时正确。shift_column增加move.saturating_sub(1)且清byteOffset，move_next_line也清byteOffset。
- Range构造及serde不校验start/end顺序；len是饱和end-start，而byte_size再加1，零长度range的byte_size为1。contains_position对end包含，tree-sitter range通常end-exclusive，使用时必须分清接口；line交叉/equality辅助只看行，不比较byte/column。无坐标溢出检查。

## 字符串驻留

- StringInterner用连续byte arena，FxHasher映射到SmallVec ID bucket后再比较bytes解决碰撞，允许非法UTF8；get字符串版失败None，lossy替代，iter跳非UTF8，iter_bytes保留。clone深复制arena/map/offsets，StringId是可外部构造及serde的u32，没有所属interner验证。
- offsets存(u32 start,u16 len)，intern以as截断，不拒绝单字符串超过65535bytes或arena/ID溢出；完整bytes已写arena但读取仅截断len，随后全长lookup不匹配可重复驻留。暂记债务，不修运行时，也不声称任意长度字符串正确。
- from_parts保留全部offsets，仅合法arena slice重建lookup；非法offset仍计入len但get返回None，重复slice不规范化，非UTF8可保留。clear保留capacity并允许ID复用，旧ID不具generation。shrink只收缩arena和offsets，不收lookup。
- 6项源码测试覆盖基本去重、只读get_id、非法UTF8、10000短字符串、合法from_parts和clear，未覆盖长度截断、坏offset或跨interner误用；尚未执行。

## 语言查询与图模型入口

完整读取languages七文件共872行，scope_graph/mod38、nodes180、edges22行，graph.rs推进1–310行，insert_ref尾部尚未读完。

- Registry固定Rust、TypeScript、JavaScript、Go、Python五配置，扩展精确匹配rs/ts/tsx/js/jsx/go/py，不含mjs/cjs/pyi，大小写不规范化。TS/TSX共同使用TSX grammar，JS/JSX独立JS grammar；TS与JS不是同一family。extensions_same_language先比较扩展字符串，相同未知扩展也true。supported_extensions由HashMap迭代，顺序不稳定。
- 查询hash按primary ID排序，只hash ID和query字符串，不包括grammar版本、namespace或extensions；DefaultHasher结果不是跨Rust版本稳定格式承诺。TSLanguageConfig公有构造不校验重复/空ID，primary空为unknown，symbol_id_of扫描namespace首匹配，compile_query每次直接编译返回错误。
- Rust定义query包含ADT/type alias(class)、function、declaration_list下method、trait(interface)、mod、macro、const/static(variable)，function/method可重叠。引用包含direct/member/scoped calls、macro、impl trait/type、use及部分use alias、参数/返回/字段/let/generic/scoped/reference/tuple/struct type场景。不是所有局部let变量或任意use嵌套都被定义捕获。namespace列表缺class/method/macro等捕获类型，SymbolId可能None，后续提取实现待核对。
- TS定义含签名/abstract/class/interface/type/enum/module、变量、arrow、任何call_expression赋值均标function（不限React），并与变量capture重叠；包含列出的数组/对象一层解构、for-in和required_parameter，不意味着任意嵌套/可选/rest模式覆盖。引用包括全部type_identifier（也可能定义位置）、new、named/default import及alias、JSX名字/对象、direct call、member对象、heritage、exports和array元素；method调用捕获对象而不是property，和JS不同。
- JS定义class/function/method、arrow、普通var/lexical；引用direct及member property call、JSX、named/default import与alias、export、array元素。没有TS那组解构或全变量引用；JS named alias同时可匹配原name reference。
- Python仅class/function定义及direct/attribute call引用；namespace虽有variable/module，query并不提取赋值或import。
- Go定义function/method/type/const/var，引用call/member、全type_identifier、qualified package/type及import字符串；import alias original是整个interpreted_string_literal，包含引号，是否去引号尚待提取层。没有短变量声明的定义capture。
- 所有语言query都没有明确scope capture；作用域是否由definition范围构造须核对graph提取实现，不能把crate名字当完整语义scope解析。
- SymbolId是namespace索引与symbol索引，name越界None；LocalDef/Import/Reference.name直接按Range切src，坏range可panic，不负责边界校验。NodeKind.range对Def返回scope范围，而identifier_range返回名字范围，其他类型两者相同；此区别会影响检索。
- Graph初始化仅root scope，node_by_range按图插入顺序取首个包含请求范围的def/ref/import（Def使用scope）；tightest_node_for_range反向要求定义scope处于请求范围内，再选最小，而非寻找最小包含请求的定义。scope插入按既有嵌套图找到parent，不重新整理旧节点；重叠/插入顺序影响树形。local/global/hoisted def分别入当前/root/parent scope，范围不在root的local/import静默不入图。
- 已读insert_ref部分遍历当前至root全部同名定义/导入，只有双方SymbolId namespace不同排除；不按symbol_idx匹配，也不因近层命中停止，因此不是严格shadowing解析。完整尾部与查询消费待续。

下一段从graph.rs311行继续；本阶段未运行动态测试，codebase-graph保持pending。

## scope_graph 全部阅读完成

graph.rs311–1726行已完整读取（含3项测试），scope_graph整个模块完成。

- insert_ref仅有候选时存节点，连接所有同名可用namespace候选，不保留unresolved；insert_ref_unconditional只存孤立节点。get_references_with_definitions只返回首条RefToDef边，不返回import解析或所有候选。
- 实际query构图只处理name.definition三段、name.reference三段及alias对，忽略definition.*大范围capture；所有def进root，所有ref无条件插入而不解析边，没有子scope或Import节点。故build_scope_graph输出不是词法作用域解析结果。from_symbols还丢弃传入名字，root用首def或ref的range，不是全文件范围，lang为空。
- extract_symbols_fast只按capture前缀分类，不使用lang_config或SymbolId，不构图；重复capture不去重，alias逐match最后一对，全都lossy UTF8原文本，Go import引号不会剥掉。所谓2–3倍性能只是注释，本次未测。
- 跨文件ScopeGraphIndex以同一interner存路径与符号，插入occurrence不去重；行号外部usize饱和u32，但允许0，add_file先start_line+1再饱和。add_file覆盖graph但追加occurrence，不自动remove旧项，也不更新meta或alias。file_count/indexed_files按meta，而is_indexed按graphs，可能不同；load后graphs为空。
- 路径用to_string_lossy驻留，不canonical；非法UTF8路径可能合并。update_file_meta读取失败保留旧meta。remove_file靠reverse索引删graph/meta/occurrences，不删alias或arena字符串。rename_file移graph/meta并改occurrence路径，但覆盖目的reverse sets，不清目的旧occurrence；目标已有数据时可能破坏后续remove完整性，须由调用方约束，暂记债务。
- alias全局alias->original，不以文件/语言分区。重复alias改original后未清旧reverse；remove_file不撤销alias。definitions查询直接加一跳original，不递归；references直接+一跳original+当前symbol的reverse别名，不遍历同级别名或传递闭包，可重复、顺序受hashset影响。has_definition只看直接map，不查alias。
- smart definition去重(path,line)，按语言family优先再path排序，不考虑目录距离、scope、import来源或line；无context extension则保持原顺序。smart references不去重，只排序。extension过滤精确列表，空列表全返回；top refs按原始occurrence数，tie无稳定次序。
- compact只shrink occurrence Vec与arena/offset capacity，不移除孤儿字符串或alias；不属于垃圾回收。stats按meta文件数及occurrence数，不是唯一symbol数。QueryVersion Legacy总需重建，Version仅hash比较。
- 二进制格式SGIX/u16版本1，little endian，含arena、offset、defs、refs、alias、meta及可选query版本；不序列化graphs，重建reverse索引。write长度多处as u32截断，HashMap迭代使字节序不稳定。save直接create截断+BufWriter flush，无原子rename/fsync；管理层是否包装待读。
- load头不足4字节是IO错误；4字节非magic返回None，并不读取legacy bincode。read_from严格版本1，但按未验证计数先分配，未核验ID存在、offset合法、mtime纳秒有效、重复key或尾部余字节；query tag读取任意错误回退Legacy，未知tag也Legacy，tag1不足hash才报错。缓存输入不能当已验证可信结构或资源限额安全。
- 3项测试只覆盖u32行号边界/溢出及compact前后stats，不证明容量实际收缩、serde恶意输入安全或alias增量正确；尚未运行。

下一阶段manager构建/缓存/锁，再导航与index_manager。

## manager 全模块阅读完成

完整读取mod14、cache106、lock539、builder507行。

- cache默认root/.goto_index.bin；load exists失败统一NotFound，再读取SGIX，非magic LegacyFormat触发调用方重建；不是旧格式转换。save仅委托直接截断写，无原子包装、自动锁或父目录创建。save_index_async接收拥有的index，新建std线程丢弃JoinHandle，错误warn，无完成通知/取消/合并。cache_exists/size不验证文件类型或可解析性。
- 锁key canonical失败保留原路径；in-memory DashMap entry计shared reader/exclusive，只有同进程完整互斥。Load不读/写跨进程lock文件，因此不阻止其他进程写。Exclusive先读文本判stale再fs::write，无create_new/flock/原子竞争协议，两进程可同时成功。guard先释放内存再删lock文件，不验证文件owner，旧guard可能删后来者文件。
- stale阈值取请求操作而非文件记录的holding操作：Load/Save120秒、Build600秒、Refresh300秒；超时即使PID活也接管，无heartbeat。未来timestamp当年龄0；Unix kill(pid as pid_t,0)==0才alive，EPERM也判dead，PID复用/超范围cast不校验；非Unix一直alive仅靠超时。is_operation_in_progress同样只exclusive查文件，结果不是操作真实终态证明。
- lock文本解析接受任意operation、未知行忽略、重复字段覆盖（started无效重复不覆盖既有效值），未限制文件大小或epoch加法范围；损坏/读取失败均尝试覆盖，create_dir失败warn后仍write；write错误表示Busy io_error而非独立错误类型。epoch前生成时间unwrap可panic。6项测试仅同进程锁关系/文件创建删除，不验证跨进程竞争。
- Builder默认N-1最少1线程、chunk100、batch5000、hidden skip及gitignore true。git2打开root取得非空支持语言tracked列表就直接返回，不加untracked，不应用hidden/gitignore开关，不核对entry普通文件/存在性；与collect_files注释“也加untracked”矛盾。只有git失败或无支持tracked文件才walk，因此无tracked支持语言时反而可索引untracked。
- fallback WalkBuilder配置hidden/git_ignore/global/exclude，threads=min(n,12)，错误静默跳过；仅path.is_dir过滤，不强制regular。参数with_threads/chunk/batch无验证，chunk0会在非空par_chunks panic，batch0且chunk0亦panic，不变成IndexError。所有paths先完整收集，merge batch只限制中间FileSymbols数量，不限制单文件符号量或总索引内存。
- build_fast新建Rayon pool和全新默认registry，不沿用self.registry作解析（它仅用于文件收集/hash）；现有LanguageRegistry公开构造能力有限，先记录结构事实。parser/query按线程lang ID缓存；grammar set失败忽略、query编译失败替换空query，所以某语言失败可能仍入meta而零symbols，不返回IndexError。
- 实际IO是metadata、打开读前8000bytes判NUL，再fs::read全文件，非mmap/zero-copy。大小先检查MAX_INDEXABLE_FILE_SIZE及非空；读取中变大无再次大小限，复用读取前metadata，二次打开存在时间窗口；不拒绝tree-sitter error nodes，不设解析deadline。阈值具体值待index_manager入口核对。
- 快速提取按capture前缀记录1-based行，lossy UTF8、重复capture保留、alias原样全局汇入。只存occurrence及meta，不存graphs，所以build后file_count可>0但is_indexed仍false。最终设置queryhash并compact；空列表也设置hash。没有build内部workspace lock/cache写，协调由调用方承担。

下一阶段navigation和index_manager；本包仍pending，未运行动态测试。

## navigation 全部阅读及actor入口

完整读取navigation844行及测试，index_manager读取1–330行，shutdown方法尾部尚待续。

- Navigator持Arc index，index_mut用make_mut，共享时深clone；固定新Registry。位置查询每次按传入path读取当前磁盘全部bytes并新建parser，没有索引root拼接、缓存freshness或5MiB限制。row/col0报PositionOutOfBounds，其他文件外坐标通常NoSymbol；读失败统一FileNotFound，丢具体IO原因。
- row/col减1直接tree-sitter point，column是byte列而非UTF16/Unicode字符列。节点递归使用point>end排除，故包含end；按child顺序首命中，不验证named flag而是kind白名单。Python attribute整体也可返回，可能是foo.bar而非单名字；无identifier的空白/标点通常None，但end包含可选到前一个标识符。只匹配token处UTF8，文件其余非法字节不一定拒绝，语法error tree不整体拒绝。
- goto_def按提取的名字调用全局smart索引，不刷新索引；goto_refs直接查名字/alias，并没有注释所说“先解析到定义”。include_definition按path,line检查是否已有后逐项insert(0)，定义块顺序会反转，已有引用保留其matched symbol且不提升位置。引用自身重复保留。by_name不读文件，context只用于排序。root Location是path String,line,optional symbol，没有Range/column或serde derive，不同于types::Location。
- 9项navigation测试：Rust位置取名字、TS多种解构/参数、member对象引用、TSX依赖数组引用，使用临时非Git目录builder。主要断言存在和指定行，不覆盖多语言消歧、alias来源、end坐标或include_definition顺序。尚未执行。
- index_manager MAX_INDEXABLE_FILE_SIZE明确5*1024*1024。root FileEvent为kind+Vec<PathBuf>结构，rename约定from/to，公有new不校验path数；与types enum不混用。
- actor命令面包含单/批文件事件、rebuild、snapshot、位置/名字查询、background stale/deleted列表、filecount/stats/queryversion/hasdefinition、shutdown；SymbolLocation路径仅注释相对，new无验证。QueryError不含PositionOutOfBounds，具体坐标行为待实现。
- 已读handle send_events空批直接Ok；snapshot发command后oneshot expect，响应方退出会panic，不只是SendError；轻量统计及hasdefinition返回Option折叠失败，blocking_recv在异步runtime调用存在限制，尚未看到超时。actor线程及生命周期去重具体实现待读，不能凭顶部“at most one”注释确认为事实。

下一段index_manager331行继续，包保持pending。

## index_manager 启动与命令阶段

完整续读331–1300行，背景刷新函数主体尚待。

- handle位置/名字查询async与blocking均oneshot expect，发送成功不保证响应，关闭后可panic；无请求deadline。shutdown仅入队无join完成确认。Config默认load/save均true，cache_path自定义只改文件位置，非工作树身份。
- spawn以canonical root的DashMap entry原子复用可upgrade的Weak<Arc handle>，复用忽略后来config差异。不检查已有actor是否已shutdown，活handle可能指向断开的channel；IndexManagerHandle自身Clone复制Sender而非Arc登记身份，Weak死亡也不必等于所有Sender已消失，不能扩大成所有情况下唯一活actor保证。
- 新建unbounded command channel，启动std线程；caller立即得到handle，load/build完成前命令排队，无队列限额。load直接读cache无workspace Load锁，query版本不符或加载错误就fresh build；build错误转空Legacy index，没有向handle返回初始化失败。fresh立即save（受配置控制），缓存命中先供查询再独立后台validate。线程spawn失败expect panic，无JoinHandle暴露。
- actor自身sender在初始化和可能启动bg后drop；bg只持Weak，避免永久自持sender。测试ExitBeacon在run_loop前创建，初始化阶段panic不触发它。ACTIVE_MANAGERS死Weak惰性清除，不主动清registry键。
- run_loop顺序recv处理，退出/Shutdown末尾save。单FileEvent会try_recv持续吸收事件/批次，遇非文件命令先flush再处理；FileEventBatch只处理自身不额外drain。持续洪峰可能延迟后续工作，事件合并具体规则待读。snapshot仅Arc clone，mutation COW整index；查询不clone整个index。
- actor位置查询与Navigator共享类似逻辑但row/col0返回NoSymbol，parser按语言缓存且set_language错误忽略；仍读调用方path不拼root，不限大小，无自动重建磁盘变化。names查询直接smart，include_definition逐个insert0反转新增定义块。
- should_index只语言支持且relative path无hidden dir，不用gitignore，root外路径不containment拒绝；具体hidden predicate待读。Removed也过该gate，与builder可索引tracked hidden目录事实不同，可能残留。updates_processed计尝试不是成功，不清零。
- event apply的60秒cache节流仅在处理事件时检查，没有timer，安静后未满间隔的改动要等后续事件或shutdown；rebuild/background save不受此节流。所有save直接同步阻塞actor，无workspace Save锁/原子文件包装。
- reindex先remove旧occurrence/meta再做语言、metadata、大小1..5MiB、binary/read/parse，失败保持缺失直到下一成功事件；alias仍未清。内容再读取前metadata复用，有增长/替换窗口；query失败空query，parserNone跳过；只存symbols/meta无graph，不是tree-sitter incremental edit（每次parse None）。
- process_background_refresh删除直接按传入字符串键，不relative转换；stale仅语言检查后reindex原路径，没有hidden gate。实际bg产出是绝对还是相对路径须继续核对。rebuild替换整个Arc，失败也会覆盖为空后save。

从1301行继续背景扫描、合并、直接提取及测试。

## index_manager 全部阅读完成

完成1301–2185行，全部actor生产逻辑及测试已读。

- background仅扫描期间持Refresh锁；发命令后guard即释放，不覆盖actor实际reindex/save。Busy只跳过无自动retry。cached path直接按cwd stat/exists，没有root.join，而builder保存相对路径；root为绝对路径时walk产出绝对路径又与相对cached_set比较，可能把已有文件当new、错误删缓存项并重复重建。若cwd与root不同还可能读取同名其他文件，需将当前缺陷明确记录，不能写成可靠root内增量验证。
- background只初次cache命中后启动一次，没有周期timer；stat用Rayon全局池，walk串行、hidden/gitignore默认开，只regular file，新路径不作relative后送actor。扫描快照到命令执行间无版本戳，过时deleted可能移除已被前一事件更新的项。handle弱引用在发送时upgrade，actor已失去登记Arc时结果丢弃；后台仍可扫描完及写lock，不是主动取消。
- intern_symbols_directly对每个capture先验证UTF8，非法range跳过，与bulk lossy替换不同；仍重复capture追加、全局alias不撤销，不是零总分配（分类Vec/interner/map仍分配），只是避开每occurrence Arc中间对象。
- coalescer精确PathBuf作key，不canonical，HashMap迭代跨路径无顺序。rename>=2拆from Removed/to Created并忽略第三及以后path；单path rename保留Renamed后reindex。Created或Modified后Removed直接删待办，与是否原已索引无关，因此已索引文件Modified+Removed同批会残留旧索引。测试把该行为当预期，只验证map为空，不验证最终索引删除正确。Removed后Created/Modified变Created，其他last-wins。
- binary只首8000bytes NUL，尾部NUL不拒；is_binary_file打开/read失败false但后续full read可再失败。hidden predicate检查全部组件含文件名，.foo.rs亦隐藏，..也匹配，单点.不匹配，非UTF8组件无法to_str则不隐藏；不是严格只隐藏目录。
- actor测试覆盖基本新增顺序屏障、queryhash、binary/size/hidden过滤、轻量stats、九项coalescer、snapshot COW、handle-drop退出。binary/oversized file_count使用unwrap_or(0)，响应失败也可通过该断言。queryhash仅比较同进程两registry，不证明跨编译稳定。
- COW测试明确前后symbol和Arc::ptr_eq不同，属于有效隔离证据。退出测试使用AtomicBool beacon确认run_loop已返回，最多5秒；bg版本用测试marker请求3秒延迟并要求actor1.5秒退出，没有单独bg-start握手，线程是否进入sleep存在调度假设。shutdown一般测试只发命令未join。所有测试尚未执行。

剩三个bin及两份integration测试，然后进行隔离测试和完整映射。

## bin 与集成测试阅读完成

完整读取 bench_file_listing.rs 233 行、bench_index.rs 64 行、code_graph.rs 394 行、incremental_memory.rs 136 行、memory_integration.rs 678 行；至此本包 manifest 与全部 Rust 已读，尚待测试终态和规范映射。

- bench_file_listing 根目录取 argv1、BENCH_REPO_ROOT、GROW_ROOT；cli/git2/git2-index 单次，其他 mode 都执行三组 warmup 加每组五次并报告均值。CLI 使用非 NUL ls-files 行拆分及 lossy 字符串，带换行/被 quote 的路径不可靠；git2 tracked+untracked 与 index-only 数据集不等，未断言数量一致就计算 speedup。错误可返回空列表而不失败退出，不能把输出当准确性能比较。
- bench_index 同样解析路径，使用 mimalloc，先打印七个扩展名 query 编译状态，但 FAILED 不阻止 build；默认 builder 后报告 files/defs/refs/aliases 和 files/sec，无额外正确性校验。
- code-graph 提供 index/definition/references/stats 及自定义 cache。index 的 _force 完全未使用，每次重建并保存；threads 直接传 builder，保存失败 exit1，build 失败 panic。其他命令 load 成功直接用缓存，不检查 query stamp/源码 freshness/repo identity，不用 actor 的后台验证；读取失败才构建并 best-effort 保存。CLI 本身不取得进程锁。
- definition/references 先 load/build，之后才验证查询参数。完整 file+row+col 优先 symbol；缺部分位置参数但有 symbol 则按名字查；相对 file 与 repo join，绝对路径不限制在 repo。JSON 输出之前已有 Loaded/Building/Saved 状态 println，因此 stdout 不是单个可直接解析 JSON 文档；错误也写 stdout。结果 JSON 含 symbol、locations[path,line,symbol:null或字符串]，无 column；空结果不报错。stats 输出 top10 引用数。
- incremental_memory 独立单测试进程：500 文件每个10定义，修改100个，stats命令作队列屏障，仅断言初始500文件及>=5000定义、RSS增量<20MiB；不检查更新后精确符号/数量。Linux /proc 和 macOS ps 读取失败或不支持时跳过 RSS 断言，shutdown仅发送不join。
- memory_integration 共13项，二进制/hidden/oversized过滤为文件数量断言；binary test名字含 memory 但无RSS检查。coalescing仅检查1文件和>=1定义，不断言最后 version_49 或实际减少重建次数。snapshot单次20MiB/20次10MiB上界，fresh/cache/compact200MiB上界，在可读取RSS时执行，不能证明内存回收或无泄漏。
- batch一致性仅比较 files/defs/refs 总数，不比较符号位置；with_build_batch_size(10/50) 受默认chunk100下限约束，注释称10或50每批并不准确。peak监视每次ps调用后再sleep2ms，并非固定2ms采样；两组顺序运行、共用进程/allocator，允许batched比unbatched高30MiB，无严格内存改善保证。
- compact roundtrip 比较统计及一个代表性 func_0_0 位置，是有限有效序列化证据。raw compact测试只add_definition而未加meta，stats.files为0，虽注释称500-file；只检查统计不变及200MiB阈值，不要求RSS下降。

上述现状和测试边界单独记录，不在本次迁移中修复运行时代码。

## codebase-graph 测试终态

会话11037退出0；macOS默认feature，cargo test --locked -p codebase-graph -- --test-threads=1 --nocapture：52单元+1独立增量内存集成+13内存集成+1 doctest=67通过，0失败，4 doctest ignored，三个bin测试目标各0项。日志/tmp/grow-codebase-graph-inventory-tests.log。实际取得RSS：增量27.9→29.1MiB；本次batch峰值21.0、unbounded45.0MiB，快照增量显示0.0MiB；这仅证明该环境样本通过既有宽松阈值，不推断所有规模性能或修复已记录边界。未运行基准CLI实仓压力测试或Linux目标。完整规范映射尚待完成，因此codebase-graph仍pending，整体36/61；git diff --check通过。

## 映射完成（前文pending为阶段历史）

全部源码已读，测试67通过4ignored；本包功能映射完成。

## 功能与规范映射

- [Graph language registry](../specs/codebase-navigation/spec.md#requirement-graph-language-registry)：语言注册表 SHALL 提供Rust、TypeScript、JavaScript、Go、Python五种配置，精确匹配rs/ts/tsx/js/jsx/go/py扩展名。
- [Graph query version identity](../specs/codebase-navigation/spec.md#requirement-graph-query-version-identity)：query版本 SHALL 由排序后的主语言ID及query文本生成，供缓存判断重建。
- [Rust graph query coverage](../specs/codebase-navigation/spec.md#requirement-rust-graph-query-coverage)：Rust query SHALL 提取所列ADT、trait、module、macro、function、const、static及选定调用和类型引用。
- [TypeScript graph query coverage](../specs/codebase-navigation/spec.md#requirement-typescript-graph-query-coverage)：TypeScript query SHALL 包含函数签名、类、interface、类型标识符、选定解构和参数、JSX及export引用。
- [JavaScript graph query coverage](../specs/codebase-navigation/spec.md#requirement-javascript-graph-query-coverage)：JavaScript query SHALL 提取其函数、类、变量及调用模式，独立于TypeScript查询。
- [Python and Go graph queries](../specs/codebase-navigation/spec.md#requirement-python-and-go-graph-queries)：Python SHALL 提取class/function定义及call引用；Go提供函数方法类型常量变量及调用类型和import相关capture。
- [Graph coordinate and range arithmetic](../specs/codebase-navigation/spec.md#requirement-graph-coordinate-and-range-arithmetic)：Position SHALL 保存byte列和offset并提供零基与一基转换；Range包含判断含end，len采用end减start。
- [Graph file metadata freshness](../specs/codebase-navigation/spec.md#requirement-graph-file-metadata-freshness)：FileMeta SHALL 以size及mtime秒纳秒比较新鲜度，不计算内容hash。
- [Graph byte string interning](../specs/codebase-navigation/spec.md#requirement-graph-byte-string-interning)：StringInterner SHALL 以byte arena及hash桶内容比较复用ID，提供byte与UTF8访问。
- [Graph public event and location models](../specs/codebase-navigation/spec.md#requirement-graph-public-event-and-location-models)：包 SHALL 同时保留actor的kind加paths事件与types中的分支枚举事件，以及导航和序列化位置模型。
- [Local scope graph APIs](../specs/codebase-navigation/spec.md#requirement-local-scope-graph-apis)：ScopeGraph SHALL 提供local/global/hoisted定义、scope/import和引用插入及范围查找。
- [Query graph construction boundary](../specs/codebase-navigation/spec.md#requirement-query-graph-construction-boundary)：query构图 SHALL 将匹配定义放到root并记录孤立引用，不从现有query构造完整scope/import或RefToDef解析边。
- [Fast graph capture extraction](../specs/codebase-navigation/spec.md#requirement-fast-graph-capture-extraction)：fast提取 SHALL 按capture前缀分类定义引用和alias，不自动去重。
- [Graph occurrence and file accounting](../specs/codebase-navigation/spec.md#requirement-graph-occurrence-and-file-accounting)：索引 SHALL 分别存储occurrence、graph、file metadata；file_count与indexed_files由metadata决定。
- [Graph file removal and rename](../specs/codebase-navigation/spec.md#requirement-graph-file-removal-and-rename)：remove_file SHALL 删除该路径的graph、metadata及反向occurrence，但保留interner和全局alias。
- [Graph alias resolution](../specs/codebase-navigation/spec.md#requirement-graph-alias-resolution)：alias SHALL 在全局名字空间记录单跳原名及反向关系，定义查原名，引用纳入原名和直接alias。
- [Graph name lookup and ranking](../specs/codebase-navigation/spec.md#requirement-graph-name-lookup-and-ranking)：smart定义查询 SHALL 以path与line去重，再按语言family及path排序；引用查询保留重复。
- [Graph compact binary persistence](../specs/codebase-navigation/spec.md#requirement-graph-compact-binary-persistence)：索引 SHALL 使用SGIX v1 little-endian格式保存interner、occurrence、metadata、alias及query版本，不序列化graph。
- [Graph cache input validation](../specs/codebase-navigation/spec.md#requirement-graph-cache-input-validation)：load SHALL 对不足四字节返回IO错误，其他非magic返回None，读取计数驱动结构恢复。
- [Graph cache wrapper lifecycle](../specs/codebase-navigation/spec.md#requirement-graph-cache-wrapper-lifecycle)：cache包装 SHALL 提供默认路径、存在与大小探测、同步load/save及后台保存。
- [Graph workspace lock semantics](../specs/codebase-navigation/spec.md#requirement-graph-workspace-lock-semantics)：try_lock SHALL 在同进程协调共享Load及互斥操作，独占操作另检查并写磁盘锁。
- [Graph stale lock policy](../specs/codebase-navigation/spec.md#requirement-graph-stale-lock-policy)：磁盘锁 SHALL 按请求操作timeout与pid存活判断是否可替换。
- [Graph build enumeration](../specs/codebase-navigation/spec.md#requirement-graph-build-enumeration)：IndexBuilder SHALL 优先采用git2非空受支持tracked列表，否则回退walk。
- [Graph build resource controls](../specs/codebase-navigation/spec.md#requirement-graph-build-resource-controls)：builder SHALL 默认N减一且至少一线程、chunk100、batch5000，batch有效大小不小于chunk。
- [Graph parsing eligibility](../specs/codebase-navigation/spec.md#requirement-graph-parsing-eligibility)：解析 SHALL 预检非空且不超过5MiB，首8000bytes NUL判binary，然后全文件读取与tree-sitter解析。
- [Graph disk position navigation](../specs/codebase-navigation/spec.md#requirement-graph-disk-position-navigation)：Navigator SHALL 从当前磁盘文件与byte行列查标识符，再查询缓存名字索引；名字查询无需读文件。
- [Graph actor identity and channels](../specs/codebase-navigation/spec.md#requirement-graph-actor-identity-and-channels)：IndexManager SHALL 以canonical root的Weak Arc表复用handle，使用无界队列串行处理命令。
- [Graph actor query responses and snapshots](../specs/codebase-navigation/spec.md#requirement-graph-actor-query-responses-and-snapshots)：actor SHALL 提供blocking及async查询、轻量统计和Arc快照，更新通过COW隔离现存快照。
- [Graph actor initialization](../specs/codebase-navigation/spec.md#requirement-graph-actor-initialization)：actor SHALL 在启动时加载cache并比较query版本，失败或不匹配则fresh build；构建失败回退空Legacy索引。
- [Graph incremental event coalescing](../specs/codebase-navigation/spec.md#requirement-graph-incremental-event-coalescing)：actor SHALL 合并连续文件事件，遇到其他命令先flush；rename至少两path拆Removed和Created。
- [Graph incremental replacement](../specs/codebase-navigation/spec.md#requirement-graph-incremental-replacement)：增量 SHALL 按language与全部路径组件hidden过滤，先删旧项再读取解析。
- [Graph cache save scheduling](../specs/codebase-navigation/spec.md#requirement-graph-cache-save-scheduling)：actor SHALL 在事件处理时检查60秒保存间隔，另在重建、后台刷新、shutdown路径保存。
- [Graph background validation limits](../specs/codebase-navigation/spec.md#requirement-graph-background-validation-limits)：cache命中后 SHALL 启动一次后台扫描，持Refresh锁扫描后发命令，Busy时跳过无retry。
- [Graph CLI command and cache policy](../specs/codebase-navigation/spec.md#requirement-graph-cli-command-and-cache-policy)：code-graph SHALL 提供index/definition/references/stats及自定义cache，index每次重建，force参数不影响行为。
- [Graph CLI output format](../specs/codebase-navigation/spec.md#requirement-graph-cli-output-format)：CLI SHALL 输出symbol及path/line位置，references可包含alias名，stats列top10引用。
- [Graph benchmark entrypoints](../specs/codebase-navigation/spec.md#requirement-graph-benchmark-entrypoints)：bench_index SHALL 打印query编译状态及默认builder统计，bench_file_listing比较CLI/git2/index-only计时。

## 边界

- 不提供对应配置；TS与TSX共用TSX grammar，TS和JS仍是不同family。
- 不保证hash随之改变；DefaultHasher不构成跨Rust版本稳定格式。
- 不得据此宣称完整Rust符号解析；重叠capture保留重复可能。
- 可被标为function；member调用主要引用object而非property，不提供类型推断。
- 按JS capture记录property或原始名字，不保证与TS查询覆盖一致。
- 不能假定短声明已覆盖；literal名字可保留引号，Python查询不是完整变量引用索引。
- byte_size采用差值加一；不验证反向范围或溢出，不应当作LSP UTF16坐标。
- stale判定失败侧视为过时；mtime可回落零，同size同mtime内容变化不保证检测。
- 不提供checked转换、归属或generation保证；clear可复用ID，compact不清理无引用字符串。
- 类型本身不验证arity；types rename主路径为to，位置模型不校验绝对路径。
- 按名字与namespace遍历祖先候选，不在shadowing处停止；没有候选不插入，unconditional可插入孤立引用。
- 不能将所有捕获引用视为已解析；from_symbols丢弃名字且root范围来自首个occurrence。
- bulk采用lossy文本；actor直接intern路径跳过非法UTF8 capture，两个入口边界不同。
- 不自动建立metadata；重复添加可追加旧occurrence，is_indexed检查graph而非metadata。
- 当前实现可覆盖目标反向记录而保留旧occurrence，不保证替换事务或alias清理。
- 不保证清除旧反向关系或递归解析，不按文件及语言隔离。
- 不进行语义消歧；top引用数平局及原始hash遍历顺序不稳定。
- 直接create与flush无原子rename/fsync保证；compact只缩减容量，不是垃圾回收。
- 不保证资源限额、ID关系及尾部验证；query tag读取错误或未知值回Legacy，已识别tag后截断仍可报错。
- 线程持有index且不提供join终态，错误警告；包装本身不保证锁或原子发布。
- 检查与写入不原子，不构成flock或create_new互斥；Load忽略磁盘锁，释放时无ownertoken校验。
- 阈值分别120、600、300秒，非原持锁操作阈值；超时可替换存活进程锁，Unix kill0权限错误也判dead。
- 快速路径遗漏untracked，hidden/gitignore选项不作用于该路径；walk错误跳过。
- chunk0可panic；收集及hash用配置registry，但解析使用新默认registry，不保证自定义解析一致。
- 不重新执行大小上限或拒绝error tree；无parse deadline，query失败可替换空query，metadata来自读取前。
- 不保证同一快照语义；位置读不受5MiB限制，include_definition前插定义且仅按path/line避重。
- 复用忽略新配置；弱身份不覆盖内部Sender clone，不能保证永远仅一个actor；shutdown只发送不join。
- 部分API expect会panic，轻量统计折叠为None；没有响应timeout或有界排队保证。
- 可在一次后台验证完成前服务查询；初始化load/build/save未统一受workspace lock保护。
- 当前coalescer取消待办，可保留旧索引；多余rename path忽略，跨path HashMap无稳定顺序。
- 旧项可消失，或过滤阻止删除而残留；不验证路径在root内，也不完整执行gitignore。
- 不因时间经过自动保存；更新计数累计而非每次保存重置，保存本身无锁事务保证。
- 当前stat未join root且walk为绝对路径，可能误判deleted或new；锁不覆盖actor应用，没有快照版本防止过时结果。
- 直接使用而不执行actor freshness/query检查；参数合法性检查发生在load/build之后，完整位置优先symbol。
- 结果段为JSON但stdout前有cache状态文本，不能声称整条stdout是单个JSON；错误也println且失败exit1。
- 基准仍可继续；非NUL CLI路径处理不可靠且数据集未校验等价，不能当严格性能证明。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
