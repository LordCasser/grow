# sqlite-vec 逐包核查

包路径：`third_party/sqlite-vec`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，已运行dynamic extension探针，Cargo check通过；Cargo test因非workspace成员被拒绝。

## 模块与开关

- `third_party/sqlite-vec/Cargo.toml`
- `third_party/sqlite-vec/build.rs`
- `third_party/sqlite-vec/src/lib.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [SQLite vector build and registration](../specs/vector-storage/spec.md#requirement-sqlite-vector-build-and-registration)：扩展 SHALL 通过build.rs将主C及条件include编译为sqlite_vec0并定义SQLITE_CORE；默认包含rescore/DiskANN、关闭实验IVF，Rust暴露sqlite3_vec_init入口地址供SQLite注册。
- [SQLite vector scalar and virtual table entrypoints](../specs/vector-storage/spec.md#requirement-sqlite-vector-scalar-and-virtual-table-entrypoints)：初始化 SHALL 注册vec_version/debug、距离/类型/长度/构造/算术/切片/归一化/JSON/量化函数，以及vec0和只读vec_each；SQL返回类型通过subtype携带。
- [Vector value parsing and matching](../specs/vector-storage/spec.md#requirement-vector-value-parsing-and-matching)：向量解析 SHALL 支持float32非空4字节倍数BLOB或文本、int8非空BLOB或范围内整数文本、bit非空BLOB；共同运算检查类型与维数一致。
- [Vector distance metrics and SIMD selection](../specs/vector-storage/spec.md#requirement-vector-distance-metrics-and-simd-selection)：float/int8 SHALL 提供L1、实际开根号L2、cosine，bit提供Hamming；按编译宏及维数分派SIMD或标量。
- [Vector arithmetic and slicing](../specs/vector-storage/spec.md#requirement-vector-arithmetic-and-slicing)：vec_add/sub SHALL 要求同类型同维数且拒绝bit；slice按整数转换的[start,end)返回非空子向量，bit边界必须8对齐。
- [Vector normalization and JSON output](../specs/vector-storage/spec.md#requirement-vector-normalization-and-json-output)：vec_normalize SHALL 仅处理float向量并除以L2范数；vec_to_json按float格式、整数或bit低位先行输出。
- [Public vector quantizers](../specs/vector-storage/spec.md#requirement-public-vector-quantizers)：公开量化 SHALL 以正值置bit且维数8对齐，int8 unit量化以2/255步长映射并夹取到[-128,127]。
- [Vector each relational projection](../specs/vector-storage/spec.md#requirement-vector-each-relational-projection)：vec_each SHALL 作为只读eponymous虚表要求可用的vector等值约束，按0起始rowid逐元素输出。
- [Vector table schema and limits](../specs/vector-storage/spec.md#requirement-vector-table-schema-and-limits)：vec0 SHALL 要求至少1个向量列；上限为16向量、4partition、16auxiliary、16metadata、8192维；默认chunk1024，显式chunk为正8倍数且不超过4096。
- [Vector table identity and command columns](../specs/vector-storage/spec.md#requirement-vector-table-identity-and-command-columns)：vec0 SHALL 支持默认整数rowid或单个显式INTEGER/TEXT主键；新表加入表名hidden命令列及distance/k hidden。
- [Vector shadow storage and chunk layout](../specs/vector-storage/spec.md#requirement-vector-shadow-storage-and-chunk-layout)：vec0 SHALL 将身份、chunk位置、有效位和向量/元数据分表存储；flat向量和metadata chunk显式绑定_rowid_及rowid一致。
- [Vector query planning restrictions](../specs/vector-storage/spec.md#requirement-vector-query-planning-restrictions)：MATCH SHALL 优先选择KNN，必须LIMIT或k等值二选一；ORDER BY可省略，提供时只能单个distance升序；ID等值否则选point，剩余fullscan。
- [Flat vector KNN candidate filtering](../specs/vector-storage/spec.md#requirement-flat-vector-knn-candidate-filtering)：flat KNN SHALL 校验向量类型维数及k在0..4096，按partition选chunk、validity与rowid IN及metadata求交后计算距离，再应用距离范围、逐chunk合并top-k。
- [Metadata filter representations and limitations](../specs/vector-storage/spec.md#requirement-metadata-filter-representations-and-limitations)：metadata过滤 SHALL 按bool位、i64、double和TEXT prefix/长文本表示处理；partition比较交给shadow SQL，text比较使用strncmp。
- [Vector cursor and column retrieval](../specs/vector-storage/spec.md#requirement-vector-cursor-and-column-retrieval)：fullscan SHALL 按chunk位置扫描身份；point读取全部向量，KNN按k_used输出；Column返回对应ID、vector subtype及其他字段，普通查询distance为NULL。
- [Vector insert type and conflict behavior](../specs/vector-storage/spec.md#requirement-vector-insert-type-and-conflict-behavior)：INSERT SHALL 校验主键、partition与向量，拒绝非NULL hidden distance/k，OR REPLACE先删除现有身份后插入；随后写索引、auxiliary、metadata。
- [Vector updates and null sentinel](../specs/vector-storage/spec.md#requirement-vector-updates-and-null-sentinel)：UPDATE SHALL 拒绝partition修改及TEXT主键变化，支持flat/rescore向量更新，拒绝DiskANN/IVF非NULL向量更新；NULL向量作为跳过标记。
- [Vector deletion and empty chunk reclamation](../specs/vector-storage/spec.md#requirement-vector-deletion-and-empty-chunk-reclamation)：DELETE SHALL 删除身份与auxiliary、清有效位/rowid/向量/metadata及长文本，调用各索引清理并回收全空chunk。
- [Vector table lifecycle and rename](../specs/vector-storage/spec.md#requirement-vector-table-lifecycle-and-rename)：vec0 SHALL 在Sync释放缓存statement，在Disconnect释放实例；Rename重命名各现有shadow表并更新缓存，Destroy按索引删除shadow表。
- [Rescore storage and quantization](../specs/vector-storage/spec.md#requirement-rescore-storage-and-quantization)：rescore SHALL 为float列保存量化chunk和原float KV，支持bit或int8；插入/更新同步两份，删除及chunk回收清理对应数据。
- [Rescore candidate reranking and filter gap](../specs/vector-storage/spec.md#requirement-rescore-candidate-reranking-and-filter-gap)：rescore SHALL 先量化扫描最多min(k*oversample,4096)候选，再读取原float按配置metric重排取k；支持rowid IN。
- [Rescore runtime oversample command](../specs/vector-storage/spec.md#requirement-rescore-runtime-oversample-command)：oversample=命令 SHALL 按atoi读取正整数并修改所有rescore列的内存搜索override。
- [DiskANN node graph and beam search](../specs/vector-storage/spec.md#requirement-diskann-node-graph-and-beam-search)：DiskANN SHALL 保存原向量与有效位/邻居ID/量化邻居blob，从_info medoid进行有界候选搜索，结合量化邻居探索和原向量读取确认。
- [DiskANN insertion pruning and buffering](../specs/vector-storage/spec.md#requirement-diskann-insertion-pruning-and-buffering)：DiskANN SHALL 先存原向量，按threshold直接建图或buffer累计后flush；选邻居用alpha prune，反向边满时量化最差替换。
- [DiskANN deletion and runtime settings](../specs/vector-storage/spec.md#requirement-diskann-deletion-and-runtime-settings)：DiskANN DELETE SHALL 清buffer或修复图邻居、删节点/vector、调整medoid并scrub残留引用；search_list_size系列命令设置首个DiskANN列。
- [DiskANN query constraint differences](../specs/vector-storage/spec.md#requirement-diskann-query-constraint-differences)：DiskANN KNN SHALL 走独立类型维数验证路径，k<=0空结果；其当前实现未使用公共4096上限及planner下推的distance/rowid IN。
- [Experimental IVF configuration and cells](../specs/vector-storage/spec.md#requirement-experimental-ivf-configuration-and-cells)：显式启用IVF宏后 SHALL 提供nlist/nprobe、none/int8/binary量化及oversample；存centroid、64槽cell、rowid_map及量化时原float KV，未训练数据归-1。
- [Experimental IVF training and commands](../specs/vector-storage/spec.md#requirement-experimental-ivf-training-and-commands)：IVF SHALL 提供compute-centroids、set-centroid、assign-vectors、clear-centroids与nprobe内存命令，默认作用第一个IVF列。
- [Experimental IVF search and mutation limitations](../specs/vector-storage/spec.md#requirement-experimental-ivf-search-and-mutation-limitations)：IVF SHALL 扫未分配或最近nprobe中心的cells，以量化距离排序，量化且oversample>1时读取原float重排候选。
- [IVF kmeans initialization and iteration](../specs/vector-storage/spec.md#requirement-ivf-kmeans-initialization-and-iteration)：聚类 SHALL 使用xorshift32和kmeans++初值，seed0换42、k裁到N，以平方L2分配并求均值，无分配变化即停止；空cluster重置最远样本。

## 边界

- SQLite以三参数int返回值调用实际C入口；Rust无参数声明用于取地址，不作为直接调用约定。内建Rust测试仅检查vec_version以v开头，不证明所有SQL行为。
- 版本为v0.1.10-alpha.4；实验IVF需显式编译宏，Cargo feature列表不提供启用开关。注册失败返回错误，不声明逐项注册回滚。
- 当前float/int8解析接受；空向量、float NaN/Inf、int8越界拒绝。文本语法不是严格JSON验证器，未知subtype拒绝。
- 没有零范数保护或clamp；int8 L1使用i32累计。SIMD启用由C编译条件决定，不由Rust feature声明保证。
- 索引截断为0与2；float结果不再做有限性验证，int8运算不保证饱和。start等于end拒绝。
- 归一化产生NaN，后续解析拒绝；不能声明零向量保持为零。float JSON采用%f表示。
- 零bit为0；rescore内部使用>=0，不能把公开函数与各索引量化视为相同。
- 输出高位先行00000001，与vec_to_json低位先行10000000不同；无vector约束拒绝查询计划。
- rescore/IVF/DiskANN拒绝metadata和partition，初始化允许auxiliary；DiskANN拒绝bit列，binary要求维数8对齐。parser部分关键字接受前缀，不能当完整SQL类型系统。
- 按_info CREATE_VERSION_PATCH>=10决定命令列；新建时vector/partition/auxiliary/metadata同表名冲突拒绝，主键未在该循环检查。显式主键声明WITHOUT ROWID。
- 只在最新匹配partition chunk找首个空槽，满时创建；不扫描更旧chunk空洞。TEXT metadata<=12字节内联，超过12字节存独立长文本。
- auxiliary过滤拒绝，boolean仅=或!=，distance仅GT/GE/LT/LE；metadata IN仅INTEGER/TEXT，rowid IN依赖SQLite>=3.38及编译支持。
- 0空结果，负数/超限报错；距离阈值转换f32。单值IN可能被SQLite简化为等值后过滤，不能保证与多值IN同样下推。
- 等值可成功而>=报Could not filter metadata fields；范围分支<12内联与写入<=12不一致。此为现状缺陷，不能声明等同SQLite完整collation语义。
- 部分Column响应nochange；point缓存向量，KNN向量按需读；xRowid对KNN返回内部错误，显示ID走Column。
- 实测失败后同连接仍能看到新rowid/vector，不保证语句原子回滚；FLOAT metadata不接受INTEGER，auxiliary INSERT允许NULL或精确类型。
- 向量保留原值，auxiliary UPDATE不检查声明类型，大整数BOOLEAN经32位转换可接受并存0；这些边界不视为严格类型保证。
- 对应chunks、flat/rescore/metadata chunk行移除并失效latest缓存；不承诺故障中途清理原子性或损坏状态的完整检测。
- 这些回调本身为空OK，不证明完整撤销；ShadowName静态登记core/metadata，未覆盖所有vector/index后缀。
- rescore使用>=0得到FF，公开binary函数得到00；int8为unit步长夹取，不同于IVF乘127。
- 当前distance条件被planner omit但未执行，可能返回不满足WHERE的结果；重排仅覆盖候选，不保证全集精确top-k。
- 前两者接受，0拒绝；不同于schema上限128，无rescore列也可返回OK，设置不持久化。
- 重复候选仅接受更小距离，故confirmed可能保留近似值；[3]*8到零实测返回8而实际L2约8.485。node_read严格校验blob尺寸，其他路径不等同完整损坏库保护。
- buffer仍参与KNN完整距离合并；删除medoid选择第一个其他原向量为入口。flush/反向边部分错误处理不保证全部成功或回滚。
- 参数atoi且无独立上限、不持久化；节点读取失败在删除入口可能按不存在返回成功，不能宣称所有失败被传播。
- 4097接受，IN可能返回1，distance可返回小于100值；这些已复现差异属于现状限制，不能视为SQL过滤正确支持。
- 默认nprobe夹到nlist，显式超限拒绝；none加oversample>1拒绝。int8距离固定L2，binary Hamming；实现按float32输入布局处理。
- 参数以strstr/atoi提取而非JSON解析；训练尝试savepoint但多处返回码忽略；set/assign/clear存在量化布局与stride限制，不声明这些组合正确。
- slot=n_vectors且删除只减计数，已复现覆盖存活向量；过滤参数未进入IVF查询，WHERE可能失效；point返回cell字节，不能保证量化模式输出原float。
- 返回-1；默认调用迭代25次，训练距离不随列cosine/L1改变，后续IVF分配另按列metric。分配使用SQLite allocator。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 原生逐段证据与验证历史

# sqlite-vec 逐包核查（进行中）

包仍 pending。原清单source_files/source_lines仅统计src下Rust文件，不能代表完整FFI实现；本包有5个C实现文件、14951行，已作为native_source_files登记路径、大小及指纹，尚未标记审阅完成。

## 已读取范围

- Cargo.toml、Cargo.toml.orig、build.rs、src/lib.rs、sqlite-vec.h全部读取。
- sqlite-vec.c只读取1–190、1295–1599、10580–10716行；中间算法、解析器和vec0 CRUD/KNN尚待审阅。第一段结束于NEON cosine声明，未读函数体。
- diskann、rescore、ivf、ivf-kmeans四个C文件尚未逐行读取。sqlite3.h/sqlite3ext.h是SQLite API头，后续按调用涉及接口核对，不当作本仓库自产数据库实现。

## 已确认事实

- build.rs以cc编译sqlite-vec.c为sqlite_vec0，定义SQLITE_CORE，只在编译器支持时添加-Wno-unused-parameter。未在脚本里开启AVX/NEON，外部CFLAGS是否影响实际构建需验证。
- 头文件版本v0.1.10-alpha.4、source commit 04d28bd21773981e2d266bbf6aa4efbd011eb4f6。vendoring说明称四个补充C文件与上游一致；本轮未联网比较，不把说明当字节比对证明。
- C默认SQLITE_VEC_ENABLE_DISKANN=1、SQLITE_VEC_ENABLE_RESCORE=1、SQLITE_VEC_EXPERIMENTAL_IVF_ENABLE=0；include路径显示diskann/rescore在对应条件下编入，IVF及kmeans默认不编入，但仍须枚举可选功能。
- Rust只暴露无参数无返回值的unsafe extern sqlite3_vec_init符号；C真实签名为int(sqlite3*,char**,api*)。唯一Rust测试把符号地址transmute为sqlite3_auto_extension函数指针，开内存连接查询vec_version并只断言前缀v。没有安全Rust注册包装或向量算法测试。本轮未运行测试。
- init注册vec_version、vec_debug及16个标量函数：distance_l2/l1/hamming/cosine、length/type/to_json/add/sub/slice/normalize、f32/bit/int8、quantize_int8/quantize_binary（均为vec_前缀）；注册vec0和vec_each两个virtual table模块。
- 注册DEFAULT_FLAGS为UTF8|INNOCUOUS|DETERMINISTIC，多数向量函数加SUBTYPE，构造/转换函数加RESULT_SUBTYPE；vec_type注册不加SUBTYPE。version/debug注册失败直接返回rc，后续function/module失败还写pzErrMsg；未实现已注册对象的显式回滚。
- debug字符串包含版本、日期、commit和实际预处理flags的avx/neon/rescore/ivf/diskann标记。
- ensure_vector_match依次解析两个输入，失败附第几个输入上下文并清理已分配值，要求element type和dimensions一致；不一致返回SQLite error。
- vec_f32/bit/int8返回blob并设223/224/225子类型，vec_length返回维数int64。具体接受文本/blob格式还需读取解析器。
- cosine、L2和L1接受float32/int8，bit报错；hamming只接受bit，float32/int8报错。完成或类型拒绝路径均cleanup双方。int8 L1调用处暂存为i64并交付sqlite3_result_int；补读helper后确认其实际返回与累计均为i32，不能将问题归结为本次交付才缩窄。大维数累计边界仍需结合调用上限核查。

## 下一步

读取主C文件剩余距离实现、输入解析、标量转换、vec_each、vec0 schema/索引规划/存储/查询/事务，再遍历四个include实现，按真实默认和可选开关形成delta。未完成前不运行生成器把整个包标为reviewed。

## 已完整读取文件指纹

- `third_party/sqlite-vec/Cargo.toml` SHA-256 `514fab031b38ea4f38bd0a888a19ef8e382eb4e48b852b9ac33ead4fb95401ae`
- `third_party/sqlite-vec/Cargo.toml.orig` SHA-256 `2d40fbcc75e4d6f057b5d924fee74d2f06ce5e4df0a0f12f5a504cd83b82811c`
- `third_party/sqlite-vec/build.rs` SHA-256 `69bdc9cbf1c9e05888e4f1b48c06b11a157288362d02c522bc2737403d09747b`
- `third_party/sqlite-vec/src/lib.rs` SHA-256 `f8ea6d5505c8bf34e36cb40c707320867425d98e5d09dda2fe3571237a00e341`
- `third_party/sqlite-vec/sqlite-vec.h` SHA-256 `7c17d6150e89be3525ae8ec0071f001ad8065b05c484cbfc711d8ce722843a81`

## 输入解析与标量运算补充

本轮增加读取sqlite-vec.c 940–1295与1600–1846行（1846仍在vec_sub尾部，需续读）。

- float32 BLOB必须非空且长度可整除sizeof(f32)，复制后逐元素拒绝NaN/Inf。TEXT通过strtod解析，要求找到开头[，拒绝解析失败、特定errno以及转f32后的非有限数，但数字间空白/逗号可任意跳过，未强制闭合或尾部耗尽。
- int8 BLOB非空，直接借用输入内存；TEXT使用strtol base10并拒绝超出[-128,127]。类似地不严格验证逗号和闭合。bit只接受非空BLOB，维数为字节数乘CHAR_BIT。vector_from_value无subtype、float32或JSON subtype走float32，bit/int8走专用解析器，未知subtype返回error。
- vec_type解析向量后返回float32/int8/bit。binary quantization要求维数正且可整除8，只支持float/int8，元素>0设位，其余清零，按i%8低位优先。bit输入报错。
- int8 quantization第一参数直接走float32解析；第二参数必须TEXT且大小写无关unit、长度恰好4。按[-1,1]的255档变换，clamp到[-128,127]后cast。超范围输入不额外拒绝。
- vec_add/sub要求类型/维数一致，拒绝bit；float按元素运算，int8结果cast回i8，没有显式饱和。float运算结果没有本地NaN/Inf再检查，后续读取可能拒绝，尚未动态验证此边界。

### 本地解析探针

从当前C源码以cc -dynamiclib -O0构建临时扩展，并在Python SQLite 3.53.4内存数据库以显式sqlite3_vec_init加载。此为动态扩展模式，未定义Cargo构建的SQLITE_CORE，不能代替完整Cargo集成或平台测试。16次SQL调用结果保存parser-probe.json。

两种vec_f32/vec_int8均接受[1 2]、[1,,2]、[1,2、[1,2]trailing并得到两个元素。int8拒绝128，float32接受128；两者拒绝空数组和NaN输入。错误文字按探针原样保存，不从解析器名字推断严格JSON语义。

包继续pending；尚未审阅主C其他区段及四个附加实现，不增加完成包数。

## 切片、归一化与schema词法补充

新增完整读取主C 1847–2598行；2598止于IVF quantizer enum开头，后续仍未读。此前声明中的vec_each尚未读取，不计完成。

- vec_slice先用sqlite3_value_int取start/end，因此整数转换先于检查；拒绝负数、超过维数、逆序、相等空范围，按[start,end)复制。float/int8以元素切片，bit要求两端均可整除8。输出保留向量subtype。int8/bit分配失败路径直接return而未走公共cleanup，是否造成实际拥有内存泄漏需结合输入类型单独评估。
- vec_to_json逐元素输出：float使用%f，int8用%d，bit按低位先行；设JSON subtype。代码有NaN转null分支，但当前float解析先拒绝NaN，不能据此声称NaN输入会返回null。
- vec_normalize只支持float32，float累计平方、sqrt后逐元素除norm，无零范数保护或结果有限性检查。动态扩展探针确认[0,0]返回NaN blob，随后vec_to_json报错；[3,4]得到[0.6,0.8]。
- 8次标量探针保存sqlite-vec-scalar-probe.json：除上述归一化，覆盖普通/小数/空范围切片与bit对齐。仍是先前动态扩展构建、Python SQLite 3.53.4，不是Cargo集成验证。
- vec0 tokenizer识别ASCII字母开头identifier（后续字母/数字/_）、纯数字、+、[]、=、()和逗号，跳过空格/tab/LF/CR；不接受引号、负号或Unicode标识符。scanner前进到token.end。
- table option解析期望key=value且结束；partition与primary key支持text/int/integer；aux列以+开头，支持text/int/integer/float/double/blob；metadata支持boolean/bool、int64/integer64/integer/int、float/double/float64/f64和text。
- 多处检查使用“token结果不对且类型不对”的AND，非逐项严格拒绝；关键字比较采用token自身长度的strnicmp，部分声明解析不检查末尾EOF。尚未动态验证schema接受边界，不能按注释推断严格完整语法。
- vec0_distance_full按float/int8与L2/cosine/L1分派，bit始终Hamming，未知组合返回0；index类型有flat、条件rescore、IVF与DiskANN，rescore配置携带quantizer与oversample、oversample_search。

## 索引配置和vec_each补充

新增读取主C 2599–3480行，覆盖rescore/DiskANN配置解析、向量列解析、完整vec_each实现及vec0开头shadow定义；3480止于user column kind枚举开头。IVF具体解析函数仍在未读include文件。

- rescore仅支持float32；必须quantizer=bit/int8，oversample默认8且允许1–128，bit维数必须8对齐。选项解析遇EOF也可结束；关键字按token长度strnicmp，不能声称严格完整拼写。
- DiskANN必须neighbor_quantizer=binary/int8；n_neighbors默认72，必须正数、8倍数、<=256。search_list_size默认128，两种search/insert override默认0，显式值必须正数，统一值不可与split值混用。alpha固定默认1.2，当前parser不接受alpha选项。buffer_threshold默认0、负值拒绝；数字通过atoi，未实施统一溢出解析。需要括号，允许尾逗号。
- VectorColumn默认flat和L2，float/f32、int8/i8、bit识别不对token长度作完整相等检查；维数atoi且>0。bit不允许distance_metric；其他允许L2/L1/cosine。多个同类选项会覆盖之前值，代码没有执行注释所说的重复distance_metric报错。flat要求空括号，rescore/DiskANN按编译宏，IVF默认拒绝。
- vec_each为eponymous-only、只读模块，schema value+hidden vector。BestIndex要求可用vector等号约束，否则SQLITE_CONSTRAINT；估算cost/rows均100000。Filter释放旧vector并解析输入，错误消息被释放后只返回SQLITE_ERROR。rowid从0逐元素递增。
- vec_each float值返回double、int8返回int；bit按0b10000000 >> (rowid%8)，高位先行。与vec_to_json的低位先行相反。动态扩展4次SQL探针保存sqlite-vec-each-probe.json：0x01在vec_each最后位置为1，vec_to_json第一位置为1；无参数vec_each返回no query solution。
- vec0 shadow定义包括info/chunks/rowids/vector_chunksNN/auxiliary/metadatachunksNN/vectorsNN/diskann_nodesNN/diskann_bufferNN/metadatatextNN。TEXT primary key由整数rowid加id TEXT UNIQUE NOT NULL映射。vector_chunks的rowid PRIMARY KEY未声明INTEGER，不能当内置_rowid_别名，后续写入路径需核对双重绑定。
- 声明上限vector列16、partition列4、aux列16、metadata列16、维数8192；目前读取到常量定义，实际创建拒绝路径还需继续核查。

尚余主C 191–939与3481–10579行，以及四个include文件。已读主C连续940–3480（加1–190和10580–10716）；先前分段阅读记录保留作为过程，不表示全包完成。

## 距离辅助与动态数组补充

新增完整读取主C 191–939行，目前1–3480和10580–10716已读；剩余主C3481–10579及四个include文件。

- L2函数虽名含sqr，但标量float/int8与SIMD路径均对平方和开根号，结果为欧氏距离而非平方距离。float累加为f32，标量int8也用f32；NEON int8使用i32平方和。
- L1 float用double做差与累计；int8 helper返回i32、标量和NEON累计也为i32。纠正先前根据调用处i64临时变量推断helper为i64的记录。没有全局饱和或无限维度正确性保证。
- cosine为1-dot/(sqrt(aMag)*sqrt(bMag))，标量float和int8使用f32累计，无零范数保护或结果clamp。NEON int8使用i32 dot/magnitude，再转换float。平台计算路径有精度/大规模整数边界差异，未执行SIMD构建对比。
- 编译开启NEON时float L2/cosine维数>16使用NEON，int8 L2>7、L1/cosine>15、float L1>3使用NEON。AVX float L2只在维数为16倍数时使用，故其无tail实现由分派约束。这些不是运行时CPU特征检测。
- Hamming对XOR结果popcount。NEON维数>=128优先，AVX字节数>=32；否则维数64倍数使用memcpy加载u64并popcount，其他逐字节查表。计数最后返回f32，标量计数为int。MSVC按平台适配intrinsic，不声称跨平台实测。
- vecJsonIsspace仅识别JSON空白的tab/LF/CR/space；vtab_set_error先释放旧zErrMsg再格式化新错误。
- Array初始化将element_size*capacity转int后分配，append满时capacity*2+100并realloc64，失败返回NOMEM，cleanup清空元数据和释放。已读float/int8文本解析调用array_append没有检查其返回值，分配失败语义不能视为完整错误传播；本轮无OOM故障注入。

本轮为源码证据补充，没有重跑此前动态扩展探针。完成包数保持不变。

## vec0状态与读取路径补充

新增完整读取主C3481–4260行，目前连续1–4260已读；4260为下一函数注释起点。DiskANN include虽然在此段出现，其文件尚未阅读。

- vec0_vtab保存连接、主键类型、command列存在性、四类用户列映射、chunk大小、shadow表名及缓存statement。IVF与DiskANN缓存按编译条件存在；这些字段不是独立实现生命周期的证明。
- vec0_free_resources finalize并置空通用statement及所有索引缓存statement。vec0_free额外释放schema/table、chunk/rowid/向量与索引shadow名称和列名；当前函数体没有释放vtab本体，也没有看到shadowMetadataChunksNames数组的释放，需追踪调用方后判断完整所有权，不能仅凭函数名断言全部资源释放。
- 用户列由kind+idx数组映射，is_*检查列范围和种类；to_*_idx直接索引，要求调用方先检查。command在用户列之后，distance/k的位置随hasCommandColumn分别偏移，支持没有command的既有schema布局。
- vec0_get_chunk_position懒准备SELECT id/chunk_id/chunk_offset，绑定rowid，缺行SQLITE_EMPTY，成功返回SQLITE_OK（注释称SQLITE_ROW不准确）；可dup id交由调用方释放，结尾reset并clear bindings。rowid_from_id临时prepare并要求恰有一行，最后finalize。
- result_id对整数主键直接返回rowid，TEXT主键从映射表复制值并释放。该接口不做schema迁移。
- get_vector_data按index类型路由：DiskANN读_vectors，IVF委托cells函数，rescore从rowid键_rescore_vectors的vector blob读取，flat先定位chunk再按offset*vector byte size读取vectors blob。输出缓冲交调用方sqlite3_free；blob close失败覆盖原成功状态，具体调用方错误清理仍需继续追踪。
- partition值经rowid定位chunk后查询partitionNN，auxiliary经rowid查询valueNN；都复制sqlite3_value返回，缺行作为SQLITE_ERROR，statement在已执行查询路径finalize。

剩余主C4261–10579和四个include实现；包保持pending。本轮没有动态测试，不把结构与读取路径审阅算作CRUD/事务验收。

## 元数据读取与chunk创建补充

新增完整读取主C4261–4895行，连续1–4895已读；下一段从vec0初始化开始。四个include仍待逐行阅读。

- metadata按chunk_offset读取：boolean每bit且低位先行，integer为i64，float为double，text每项16字节view、前4字节length，<=12字节内联，否则经rowid查询metadatatextNN。读取blob关闭结果忽略；未对损坏view长度做本地校验，不能声明畸形数据库健壮性。
- latest chunk懒准备max(rowid)查询，有partition则逐列等号AND筛选；max为NULL返回SQLITE_EMPTY，缓存statement在结束后reset/clear bindings。没有在此函数扫描更早chunk空槽。
- rowids insert支持显式整数rowid及idValue映射，后者取last_insert_rowid；按SQLITE_THREADSAFE和函数可用性进入数据库mutex。主键重复改写错误信息，其他错误传播为SQLITE_ERROR；整数rowid错误描述读取stmtRowidsInsertId的db handle，须结合失败路径再核查，不据此声称已复现崩溃。
- metadata chunk尺寸为bool chunk_size/8、int/float每项8字节、text每项16字节。update_position设置chunk_id/chunk_offset并重置缓存statement，不检查受影响行数。
- new_chunk先插_chunks的size/零validity/零rowids和partition值，再为flat vector列分配零blob，其他索引跳过普通vector_chunks；接着调用rescore_new_chunk，最后为metadata列分配blob。
- flat vector与metadata chunk INSERT都显式绑定相同_rowid_和rowid，确认先前shadow双身份注释在此创建路径有实际实现。各步骤失败直接返回，本helper未开启独立事务或撤销之前步骤；完整原子性须继续沿SQLite虚表回调验证。
- 游标有fullscan、point、KNN三种计划（字符1/2/3）；fullscan清理statement，point清理全部vector数组，KNN清理rowids和distances，cursor_clear再释放相应容器并置NULL。这是局部释放逻辑，尚未核查所有入口退出路径。

本轮仅源码审阅，无新增动态测试。当前未读主C4896–10579以及四个include文件；包保持pending。

## 初始化与shadow建表补读

完整读取主C4896–5775；连续1–5775与10580–10716已读，主C5776–10579和四个include尚待审阅。

- vec0_init按vector、partition、primary key、auxiliary、metadata、table option顺序识别参数。vector最多16列、partition4列、auxiliary16列、metadata16列；主键最多一个，缺省整数rowid，至少一个vector。维数上限8192，DiskANN拒绝bit列，binary quantizer要求维数能被8整除。
- chunk_size缺省1024，显式值经atoi，必须为正、8的倍数、<=4096；重复选项后者覆盖。rescore、IVF、DiskANN均检查并拒绝metadata/partition；IVF和DiskANN注释提及auxiliary禁用，但当前初始化代码没有对应auxiliary检查，不能据注释写成已实现约束。
- 新表为隐藏命令列保留表名，逐一检查vector/partition/auxiliary/metadata名称与表名的大小写不敏感冲突；该循环未检查自定义主键名称。旧表xConnect读取_info.CREATE_VERSION_PATCH，>=10才加入命令列；缺表/读取失败默认旧格式，仅比较patch字段。
- 声明虚表时主键列在最前、其余用户列保留参数顺序，随后同表名hidden命令列、distance hidden、k hidden；显式主键使用WITHOUT ROWID。普通用户列声明不附SQL类型。
- xCreate创建_info并记录版本文本和major/minor/patch；DiskANN各列medoid初始化NULL。创建_chunks与_rowids，flat列才创建_vector_chunksNN；rescore和IVF交给各自helper，DiskANN创建_vectorsNN、_diskann_nodesNN和_diskann_bufferNN。
- metadata每列有metadatachunksNN，text另有metadatatextNN；auxiliary独立表为INTEGER PRIMARY KEY rowid与valueNN无类型列。创建过程逐statement检查并finalize，失败走vec0_free再释放vtab本体，统一返回SQLITE_ERROR（最初malloc失败直接NOMEM）。Disconnect也释放资源和本体。完整销毁/事务路径仍待核查。
- 建表探针7例存于sqlite-vec-schema-probe.json：确认隐藏列、4096接受、7拒绝、8193维拒绝、vector列与表名冲突拒绝、TEXT主键和auxiliary建表。DiskANN探针使用的列语法被解析器拒绝，因此不能验证auxiliary共存；这一例只记录失败，不作为该能力证据。探针复用本地dynamic extension，非Cargo SQLITE_CORE测试。

包仍pending，尚未生成完整delta。

## 销毁、KNN规划与metadata过滤补读

完整读取主C5776–7300；连续1–7300和10580–10716已读，7301–10579及四个include仍待完成。

- Destroy先释放缓存statement，再依次DROP chunks/info/rowids及索引对应shadow表，最后auxiliary和metadata/text表。DiskANN使用IF EXISTS，rescore通过helper并检查返回值，IVF调用drop helper未检查返回值。结束总会vec0_free，只有成功释放vtab本体；失败后资源状态与SQLite回调后续行为尚待验证。普通flat+metadata+auxiliary实测DROP后只剩sqlite_sequence。
- Open零初始化游标，Close清理计划状态再释放。BestIndex忽略不可用约束；仅一个vector MATCH和一个被识别的rowid IN。MATCH存在优先KNN，其次ID等值走point，否则fullscan。KNN必须LIMIT或k等值二选一；ORDER BY可省略，提供时只允许单一distance升序；任何可用auxiliary过滤导致错误。
- KNN将MATCH、k、rowid IN、partition、metadata、distance参数编为固定四字符索引项，并设置omit=1。rowid IN依赖编译支持及SQLite>=3.38。partition支持=、>、<=、<、>=、!=，未识别操作不下推；metadata未知操作报错，boolean仅=和!=，IN仅INTEGER/TEXT。distance仅>、>=、<、<=，等值被拒绝。
- 估计成本/行数：KNN30/10、point10/1、fullscan3000000/100000；point idxNum存colUsed。该planner未设置orderByConsumed，不能由注释推断SQLite无需排序。idxStr分配失败路径返回OK，错误清理sqlite3_str_finish的结果未释放；不声明已做OOM验证。
- merge_sorted_lists最多输出指定数量，距离相等偏向已有a列表。bitmap为低位先行，尺寸以assert要求8倍数。min_idx默认O(n*k)重复扫描，<=使单chunk同距候选偏向后下标；实验宏启用maxheap O(n log k)，不保证相同tie顺序。
- chunk迭代将partition条件以AND组合并绑定值，查询chunk_id/validity/rowids。长text查询按rowid缓存statement，返回数据生命周期依附statement。
- metadata过滤先reopen blob并严格检查大小，再读整个chunk；boolean目标经sqlite3_value_int转truthiness，integer经int64、float经double。该helper未跳过无效槽，候选validity交集须继续核查调用方。
- text按length和12字节prefix比较，EQ/NE/IN长度<=12内联，范围比较仅<12内联，12及以上转查长文本表；底层使用strncmp而非SQLite collation。范围分支长文本比较长度为nFull，扩展前缀/嵌入NUL语义需额外验证。text IN查找索引使用size_t=-1后判<0，此防御判断无效；实际可达性需核查参数构建。blob_read失败直接返回漏释放已分配rowids的局部路径可见，未注入故障测试。
- 10例查询及DROP检查存于sqlite-vec-knn-planner-probe.json。复现缺k、k与LIMIT并用、降序、auxiliary过滤、boolean范围、distance等值拒绝；允许省略ORDER BY的k查询。12字节name等值成功，KNN >=报Could not filter metadata fields，而相同普通查询成功。此缺陷登记backlog，未修改运行时代码。

当前仍pending，尚未审阅完整KNN执行、写入和索引算法。

## KNN执行与游标补读

完整读取主C7301–8490；连续1–8490及10580–10716已读，其余主C和四个include仍待审阅。

- flat chunk执行验证validity、rowids、vector blob字节数；读取整个vector chunk后从validity复制候选bitmap，rowid IN经有序数组bsearch求交，各metadata条件逐项求交，再仅对候选计算距离。float/int8使用配置的L1/L2/cosine，bit使用Hamming。distance条件目标转f32，过滤在top-k之前；每chunk选min(k,chunk_size)，与全局结果合并。
- metadata helper虽然扫描全chunk，结果最终与有效候选求交；因此正常结果不会包含无效槽，但helper仍可能访问无效槽文本。metadataBlobs数组声明/清零在若干可能goto cleanup的分配之后，而cleanup无条件遍历关闭；OOM早退出存在未初始化句柄风险，未做分配失败注入。
- 普通KNN先校验vector类型/维数，再将k转int64，拒绝负数与>4096，0返回空游标。rowid IN转换TEXT主键或整数并排序；该块局部rc遮蔽外层，错误goto cleanup可能丢失状态，需独立故障/缺失主键验证。metadata IN拷贝TEXT/INTEGER值再交helper；临时item在加入总数组前失败的资源清理仍需审计。
- DiskANN在这些公共k/过滤处理之前直接dispatch。其helper仅解析MATCH/k，类型/维数匹配后k<=0返回空，无4096上限，传给search时转int。检索后扫描buffer用完整距离加入/替换最差结果，最后排序；buffer prepare/step失败未传播，未在此helper检查buffer vector字节数。
- DiskANN helper未处理planner已omit的distance和rowid IN。12例探针已保存：DiskANN distance>100仍返回小距离，IN(2,3)返回包含rowid1；flat同多值IN正确筛到2/3。单值IN(3)被SQLite简化后两类均返回空，不能把该例当多值IN证据。DiskANN接受4097而flat拒绝；DiskANN负k空而flat报错。索引语法需INDEXED BY diskann(neighbor_quantizer=binary)，先前quantizer参数尝试失败；正确语法同时证实auxiliary可建表插入。
- rescore接收公共rowids/metadata/idxStr后交其helper；IVF只收到vector/k，过滤语义待include核查。fullscan按chunk_id/chunk_offset遍历_rowids。point转换ID后读取所有vector（未使用planner colUsed剪枝），缺失返回EOF。
- Filter每次清理旧cursor并验证idxStr四字符布局/参数数目，再分派。Next为fullscan step、point一次后done、KNN自增；EOF以k_used判断。xRowid对KNN返回内部错误，显示ID走Column路径。fullscan vector按需读并返回subtype，distance为NULL；point使用缓存vector和SQLITE_TRANSIENT，distance也NULL。fullscan/point其他列经各自value helper获取，metadata及部分point列响应sqlite3_vtab_nochange。

以上为本地dynamic extension探针与源码事实，不声明Cargo SQLITE_CORE或全部索引正确性已验证；包仍pending。

## 主C列输出、写入与生命周期读完

完整读取8491–10579；结合已读10580–10716，主C全部10716行已读。四个索引include仍未完成，包保持pending。

- KNN Column按当前结果rowid返回ID、distance、按需vector及其他列，vector带类型subtype；隐藏k/command无专门输出分支。point metadata继续使用nochange优化。
- 插入主键：TEXT必须实际TEXT，整数主键接受INTEGER或NULL自动生成。partition允许NULL或精确类型，vector全部解析并验证类型/维数；hidden distance/k不可提供非NULL。OR REPLACE先检查并删除旧行，再执行正常插入。
- 普通chunk插入只检查最新匹配partition chunk的第一个零validity bit，满/不存在则创建chunk；不会扫描旧chunk空槽。最终写入先置validity，再写各flat vector、chunk rowids、_rowids位置。全DiskANN跳过chunk分配；随后依次DiskANN insert、rescore helper、IVF insert、auxiliary、metadata。不存在本函数内的显式回滚。
- auxiliary INSERT允许NULL或声明的实际类型；UPDATE helper直接绑定值，没有同等类型验证。metadata INTEGER/FLOAT/TEXT要求实际类型精确匹配且不接受NULL；BOOLEAN要求INTEGER但判断用sqlite3_value_int，实际上按32位转换后是否0/1。float元数据不接受SQL整数，即使数值可表示。
- TEXT写入16字节view（4字节长度+12字节prefix），只有>12字节才写长文本表。更新长到长用UPDATE，短到长INSERT，长到短DELETE。确认之前12字节范围比较错误的布局证据。该helper一些错误分支未关闭已打开blob，部分write返回码被后续close覆盖，动态探针未做I/O注入。
- DELETE先DiskANN graph helper，普通chunk清validity、rowid和flat向量，rescore helper；再删_rowids/auxiliary、清metadata及长文本、回收全空chunk，最后IVF。回收删除chunk、flat/rescore/metadata blob行并清latest缓存。validity已置位检查仅右移而未与1，损坏状态检测不严格，未做损坏库验证。长TEXT删除将SQLITE_DONE转OK，避免提前结束多行删除。
- UPDATE禁止TEXT主键变化及partition赋值；先auxiliary后metadata后vector。vector=NULL直接跳过；DiskANN/IVF非NULL向量更新拒绝；flat直接写，rescore更新量化chunk和原始float表。INTEGER主键改动、hidden赋值未见同等专门检查，不推断支持。
- INSERT的TEXT命令列优先交rescore→IVF→DiskANN，均EMPTY才未知命令报错；非TEXT命令值走普通INSERT。Begin/Commit/Rollback为空OK，Sync释放缓存statement；事务语义不能据回调名称声称已实现撤销。
- Rename批量重命名core、各索引、auxiliary和metadata shadow表，再释放statement并更新缓存名称；只有tableName分配显式判空，其他缓存分配失败未逐项检查。ShadowName静态名单含core/metadata/text，却无vector_chunks和各索引shadow后缀，暂不声明防御模式完整支持。
- 15步写入探针sqlite-vec-write-probe.json：FLOAT写入整数报错后，仍可查rowid2及其vector（同连接Python默认事务）；TEXT auxiliary UPDATE成INTEGER成功；vector=NULL保留旧值；BOOLEAN=4294967296成功读回0；rename后读正常，DELETE全表后chunks/vector_chunks计数均0。此为dynamic extension实测，非完整事务/故障测试。

主C已读不等于包完成：rescore、DiskANN、IVF及kmeans四个实现和对应验证仍待处理。

## rescore与k-means完整阅读

完整读取sqlite-vec-rescore.c 687行及sqlite-vec-ivf-kmeans.c 214行；剩余DiskANN1889行和IVF1445行。包保持pending。

- rescore每列保存量化chunk（rowid PRIMARY KEY无INTEGER）和原始float表（INTEGER PRIMARY KEY）；new_chunk显式绑定_rowid_/rowid一致。插入先写量化blob再插原始vector；删除清量化位置并删除原始行，空chunk回收量化行。建表、drop、读写结果逐步返回；不增加独立事务机制。
- bit量化以>=0置位，低位先行，和公开vec_quantize_binary的>0不同；int8使用[-1,1]映射步长2/255，夹到[-128,127]再截断。向量类型/维数由主C入口验证。
- rescore_knn候选数=min(k*oversample,4096)，内存override oversample_search>0优先于schema值。第一阶段按validity及rowid IN筛选、扫描量化距离（bit Hamming、int8按列metric）合并候选。validity/rowids仅验证最小长度，未要求完全相等。
- 第二阶段逐候选blob_open/reopen读取原float，以列metric计算完整距离，选择排序后返回min(k,cand_used)。这是候选内精确重排，不保证全集精确top-k。aMetadataIn忽略（schema禁止metadata），idxStr distance条件未被应用；已复现distance>100仍返回2.83/5.66，而多值rowid IN(2,3)正确生效。
- hidden命令oversample=区分大小写，以atoi读取正整数，设置所有rescore列的内存search override；无schema的128上限、不持久化、无匹配列也返回OK。实测129及1trailing接受，0拒绝。11步探针sqlite-vec-rescore-probe.json另确认零向量rescore bit为FF而公开量化为00，UPDATE原向量后量化更新，DELETE后两张rescore表均空。
- SQLITE_VEC_TEST只暴露4个量化/大小helper，本文件不含执行断言测试。当前探针为dynamic extension，未启用该宏。
- kmeans虽注释称无SQLite依赖，分配实际用sqlite3_malloc64/free。xorshift32，seed=0转换42；kmeans++首点随机，后续按最近已选中心平方L2距离加权，全部距离零时随机选点。N/D/k非正返回-1，k>N裁为N。
- Lloyd迭代以float平方L2分配最近中心（同距先中心），无assignment变化即停，否则求均值；空cluster重置到当前分配中心距离最远的样本。常量最大迭代25/default seed0，但helper接受max_iter参数，实际调用参数待IVF读取；没有按配置cosine/L1改变聚类距离。初始化或工作区分配失败清理并返回-1。未进行IVF启用构建/聚类动态验证。

## IVF主体完整阅读

完整读取sqlite-vec-ivf.c 1445行，原生仅剩DiskANN1889行未读。默认IVF宏为0；本轮显式-DSQLITE_VEC_EXPERIMENTAL_IVF_ENABLE=1构建/tmp/grow-sqlite-vec-ivf-audit.dylib成功，不能视为默认Cargo能力。

- 参数nlist默认128、允许0..65536；nprobe默认10、显式1..65536，nlist>0时显式nprobe超nlist拒绝，默认值则夹小。quantizer none/int8/binary，oversample>=1且>1必须量化，无独立上限。key/value前缀比较与atoi延续主parser惯例。
- 固定64槽cell，centroid_id可多行并有索引；每cell validity8字节、rowids512字节、vectors64*存储尺寸。未训练centroid=-1；状态存_info并缓存，读取失败可能缓存为未训练。centroid表、cell表、rowid_map必建；量化时另存原float KV。
- IVF假设输入float32：none尺寸4D，int8 D，binary D/8。int8夹[-1,1]乘127截断（不同rescore），binary>0低位先行。int8距离固定L2，即便列metric cosine/L1；none按列metric，binary Hamming。
- cell插入选择n_vectors<64的cell并以slot=n_vectors写入，设置validity/rowid/vector后计数+1和rowid_map插入；许多blob读写/step错误忽略。find_or_create先错误调用只有三个格式实参的helper却多一个%d，再销毁并正确重新prepare；这是源码未定义行为风险，不能因本次成功称安全。
- 删除清validity、n_vectors减1、删除rowid_map及可选原float KV，未压缩后续槽，也不清原rowid/vector。因此重新插入slot=n_vectors可能覆盖存活槽，已实测。point读取cell量化字节而非原float KV，但主C Column仍声明列元素subtype；量化输出语义需单独验证。
- 全量加载：量化从原float KV读取，none从validity有效槽读取；尺寸不足或异常blob多被跳过，部分SQLite查询错误变空/成功。ivf_exec除分配失败外总返回OK，忽略prepare/step失败。
- compute-centroids加载全量float，nlist裁到N，调用kmeans默认25次/seed0，再以配置float metric分配（训练本身平方L2）。重写centroid/cell/map，量化时量化center/vector；标训练状态1。尝试SAVEPOINT/RELEASE/ROLLBACK但不检查事务命令返回，且多处DML结果忽略，不能保证训练原子性。
- set-centroid只检查4D字节长度并存原始blob，没有量化；标训练1不自动分配旧vector。assign-vectors仅处理-1 cell，按float解释数据，nearest helper传D作为字节stride（应继续独立核查），丢弃真实centroid ID使用数组索引；clear-centroids加载原float后以量化vecSize直接写cell而未重新量化。相关组合不能声明正确支持。
- 查询未训练扫-1 cell；已训练选最近min(nprobe,nlist)中心并加-1 cell扫描，收集全部有效候选、排序，量化且oversample>1才重算前k*oversample原float距离，再取k。oversample1返回量化距离，候选内存随扫描数量增长。原float查找/错误可留下量化距离混排。未接收rowid IN或distance过滤参数，主planner仍omit，已实测违反WHERE。
- 命令只作用第一个IVF列：nprobe=正整数内存修改无schema上限；compute-centroids及冒号后strstr/atoi提取nlist/max_iterations/seed（不是真JSON解析）；set-centroid:id从对应列blob读取；assign-vectors、clear-centroids。默认rescore命令优先，因此oversample=会先被rescore handler吞掉，并非IVF运行时参数。
- 10步实验构建探针sqlite-vec-ivf-probe.json：distance>100和IN(2,3)返回rowid1；删除1再插4后，rowid3的向量变成[4,4]，KNN只剩2/4，compute-centroids仍报告成功及trained=1。已独立登记债务；未测试损坏库、量化manual center组合、attached schema或故障注入。

## DiskANN完整阅读

完整读取sqlite-vec-diskann.c1889行，五个C实现总14951行均已读。仍须完成规范映射、Rust构建验证及证据清单收口，暂不增加完整包数量。

- 节点三blob：R/8 validity低位先行、R个i64邻居ID以memcpy存取、R个量化向量；init全零。read按配置精确验证三blob长度、非NULL后复制，长度不符CORRUPT；原向量read只要求非空未核对维数尺寸。
- binary量化>0，int8以2/255线性映射后直接转i8，无rescore中的夹取；非float邻居写入直接复制字节。查询只有float预量化，非float fallback仍将query当float指针，类型路径不能声明正确，尚未运行int8图查询探针。
- medoid保存在_info，空图NULL；首插入成为medoid，删除medoid选择原向量表第一个其他rowid，无重新计算中心。SQLstep错误部分被当空图。
- candidate list按距离排序、rowid去重；同rowid仅在新距离更小时更新，保留visited/confirmed。满容量淘汰最差，visited set线性探测固定容量、0作空哨兵，不记录rowid0且不扩容。
- search取配置search专用L或统一L并至少k，以medoid完整距离起步。逐最近未访问节点加载量化邻居，失败均跳过；对当前节点重读原向量计算完整距离，再调用同一个只接受更小值的insert并标confirmed。因此近似距离小于真实距离时不会被纠正，confirmed不证明返回精确距离。已有index-filter探针rowid3向量[3]*8返回8而flat约8.485可对应此路径。
- alpha prune先按候选到p距离排序，逐最近候选选取并按alpha*d(selected,candidate)<=d(p,candidate)去除，至多R；读失败候选可能已计入selected。写邻居时读失败跳过；新节点加反向边的返回值忽略。满邻居的reverse-edge实际采用量化最远替换，不是注释所称完整RobustPrune。
- 插入总先写_vectors，threshold>0则buffer写入计数达到阈值flush，否则立即graph插入。flush逐buffer建图再清空整表，迭代异常未检查终态就进入清空；中途建图失败则已写图节点及buffer可能并存，无本地撤销。查询在主C合并buffer完整距离，见此前记录。
- 删除buffer条目直接删buffer/vector；图条目先读邻居（任意读错误直接成功），修复邻居中指向被删节点的槽，尝试填一个其他邻居，删节点/vector、更换medoid，最后全图scrub清残留ID和量化值。scrub扫描只用IDs长度限制slot，未验证validity长度，完整健壮性不能由node_read校验推断。
- 命令只作用首个DiskANN列：search_list_size、search_list_size_search、search_list_size_insert，atoi正整数、内存设置，无持久化或上限。无flush命令。SQLITE_VEC_TEST导出candidate/visited helper供外部测试，无自执行断言。
- 11步sqlite-vec-diskann-probe.json确认threshold2时首行nodes0/buffer1仍可KNN查询，第二行nodes2/buffer0，运行时命令成功，删medoid1后仅rowid2且medoid2。未将此正常路径外推到故障/损坏/大参数正确性。

## 最终收口状态

本包已完成实现阅读和30项delta映射，清单现为reviewed；上文pending仅表示逐段历史阶段。全部Rust/build/manifest/扩展头和五C已阅读并记录哈希。sqlite3.h/sqlite3ext.h作为外部SQLite API声明，按auto_extension实际调用约定、blob写入返回/尺寸边界、nochange提示和版本分派阅读相关段，不将其视为本包SQLite引擎实现。

`cargo check --locked -p sqlite-vec`通过。`cargo test --locked -p sqlite-vec`退出101：requires dev-dependencies and is not a member of the workspace；没有运行该命令的Rust单测。不为绕过该限制修改workspace或依赖；本包行为证据为源码及上述默认/显式IVF dynamic extension探针。规范严格校验15项、来源符号/文件哈希和git diff --check均通过。
