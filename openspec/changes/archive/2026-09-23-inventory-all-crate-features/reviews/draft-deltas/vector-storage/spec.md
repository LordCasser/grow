## ADDED Requirements

### Requirement: SQLite vector build and registration
扩展 SHALL 通过build.rs将主C及条件include编译为sqlite_vec0并定义SQLITE_CORE；默认包含rescore/DiskANN、关闭实验IVF，Rust暴露sqlite3_vec_init入口地址供SQLite注册。

#### Scenario: SQLite vector build and registration boundary
- **WHEN** 调用方将入口注册为auto extension后新建连接
- **THEN** SQLite以三参数int返回值调用实际C入口；Rust无参数声明用于取地址，不作为直接调用约定。内建Rust测试仅检查vec_version以v开头，不证明所有SQL行为。

证据：`third_party/sqlite-vec/build.rs` — `SQLITE_CORE`。

### Requirement: SQLite vector scalar and virtual table entrypoints
初始化 SHALL 注册vec_version/debug、距离/类型/长度/构造/算术/切片/归一化/JSON/量化函数，以及vec0和只读vec_each；SQL返回类型通过subtype携带。

#### Scenario: SQLite vector scalar and virtual table entrypoints boundary
- **WHEN** 读取版本或启用某索引
- **THEN** 版本为v0.1.10-alpha.4；实验IVF需显式编译宏，Cargo feature列表不提供启用开关。注册失败返回错误，不声明逐项注册回滚。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `sqlite3_vec_init`。

### Requirement: Vector value parsing and matching
向量解析 SHALL 支持float32非空4字节倍数BLOB或文本、int8非空BLOB或范围内整数文本、bit非空BLOB；共同运算检查类型与维数一致。

#### Scenario: Vector value parsing and matching boundary
- **WHEN** 输入非严格JSON文本[1,,2]或缺右括号的[1,2
- **THEN** 当前float/int8解析接受；空向量、float NaN/Inf、int8越界拒绝。文本语法不是严格JSON验证器，未知subtype拒绝。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vector_from_value`。

### Requirement: Vector distance metrics and SIMD selection
float/int8 SHALL 提供L1、实际开根号L2、cosine，bit提供Hamming；按编译宏及维数分派SIMD或标量。

#### Scenario: Vector distance metrics and SIMD selection boundary
- **WHEN** cosine遇零向量或查询整数距离
- **THEN** 没有零范数保护或clamp；int8 L1使用i32累计。SIMD启用由C编译条件决定，不由Rust feature声明保证。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `distance_cosine_int8`。

### Requirement: Vector arithmetic and slicing
vec_add/sub SHALL 要求同类型同维数且拒绝bit；slice按整数转换的[start,end)返回非空子向量，bit边界必须8对齐。

#### Scenario: Vector arithmetic and slicing boundary
- **WHEN** slice索引传0.9与2.9或算术结果溢出
- **THEN** 索引截断为0与2；float结果不再做有限性验证，int8运算不保证饱和。start等于end拒绝。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vec_slice`。

### Requirement: Vector normalization and JSON output
vec_normalize SHALL 仅处理float向量并除以L2范数；vec_to_json按float格式、整数或bit低位先行输出。

#### Scenario: Vector normalization and JSON output boundary
- **WHEN** 归一化全零向量后再转JSON
- **THEN** 归一化产生NaN，后续解析拒绝；不能声明零向量保持为零。float JSON采用%f表示。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vec_normalize`。

### Requirement: Public vector quantizers
公开量化 SHALL 以正值置bit且维数8对齐，int8 unit量化以2/255步长映射并夹取到[-128,127]。

#### Scenario: Public vector quantizers boundary
- **WHEN** 量化包含零的float或int8向量为binary
- **THEN** 零bit为0；rescore内部使用>=0，不能把公开函数与各索引量化视为相同。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vec_quantize_binary`。

### Requirement: Vector each relational projection
vec_each SHALL 作为只读eponymous虚表要求可用的vector等值约束，按0起始rowid逐元素输出。

#### Scenario: Vector each relational projection boundary
- **WHEN** 遍历bit BLOB 01
- **THEN** 输出高位先行00000001，与vec_to_json低位先行10000000不同；无vector约束拒绝查询计划。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vec_eachFilter`。

### Requirement: Vector table schema and limits
vec0 SHALL 要求至少1个向量列；上限为16向量、4partition、16auxiliary、16metadata、8192维；默认chunk1024，显式chunk为正8倍数且不超过4096。

#### Scenario: Vector table schema and limits boundary
- **WHEN** 组合索引与普通字段
- **THEN** rescore/IVF/DiskANN拒绝metadata和partition，初始化允许auxiliary；DiskANN拒绝bit列，binary要求维数8对齐。parser部分关键字接受前缀，不能当完整SQL类型系统。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vec0_init`。

### Requirement: Vector table identity and command columns
vec0 SHALL 支持默认整数rowid或单个显式INTEGER/TEXT主键；新表加入表名hidden命令列及distance/k hidden。

#### Scenario: Vector table identity and command columns boundary
- **WHEN** 重新连接旧表或列名与新表名冲突
- **THEN** 按_info CREATE_VERSION_PATCH>=10决定命令列；新建时vector/partition/auxiliary/metadata同表名冲突拒绝，主键未在该循环检查。显式主键声明WITHOUT ROWID。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `hasCommandColumn`。

### Requirement: Vector shadow storage and chunk layout
vec0 SHALL 将身份、chunk位置、有效位和向量/元数据分表存储；flat向量和metadata chunk显式绑定_rowid_及rowid一致。

#### Scenario: Vector shadow storage and chunk layout boundary
- **WHEN** 新行需要chunk位置
- **THEN** 只在最新匹配partition chunk找首个空槽，满时创建；不扫描更旧chunk空洞。TEXT metadata<=12字节内联，超过12字节存独立长文本。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vec0Update_InsertNextAvailableStep`。

### Requirement: Vector query planning restrictions
MATCH SHALL 优先选择KNN，必须LIMIT或k等值二选一；ORDER BY可省略，提供时只能单个distance升序；ID等值否则选point，剩余fullscan。

#### Scenario: Vector query planning restrictions boundary
- **WHEN** KNN添加auxiliary、boolean范围或distance等值过滤
- **THEN** auxiliary过滤拒绝，boolean仅=或!=，distance仅GT/GE/LT/LE；metadata IN仅INTEGER/TEXT，rowid IN依赖SQLite>=3.38及编译支持。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vec0BestIndex`。

### Requirement: Flat vector KNN candidate filtering
flat KNN SHALL 校验向量类型维数及k在0..4096，按partition选chunk、validity与rowid IN及metadata求交后计算距离，再应用距离范围、逐chunk合并top-k。

#### Scenario: Flat vector KNN candidate filtering boundary
- **WHEN** k为0、负数或超过4096
- **THEN** 0空结果，负数/超限报错；距离阈值转换f32。单值IN可能被SQLite简化为等值后过滤，不能保证与多值IN同样下推。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vec0Filter_knn_chunks_iter`。

### Requirement: Metadata filter representations and limitations
metadata过滤 SHALL 按bool位、i64、double和TEXT prefix/长文本表示处理；partition比较交给shadow SQL，text比较使用strncmp。

#### Scenario: Metadata filter representations and limitations boundary
- **WHEN** 12字节TEXT参与KNN范围比较
- **THEN** 等值可成功而>=报Could not filter metadata fields；范围分支<12内联与写入<=12不一致。此为现状缺陷，不能声明等同SQLite完整collation语义。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vec0_metadata_filter_text`。

### Requirement: Vector cursor and column retrieval
fullscan SHALL 按chunk位置扫描身份；point读取全部向量，KNN按k_used输出；Column返回对应ID、vector subtype及其他字段，普通查询distance为NULL。

#### Scenario: Vector cursor and column retrieval boundary
- **WHEN** UPDATE请求未变化列或KNN使用xRowid
- **THEN** 部分Column响应nochange；point缓存向量，KNN向量按需读；xRowid对KNN返回内部错误，显示ID走Column。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vec0Column_knn`。

### Requirement: Vector insert type and conflict behavior
INSERT SHALL 校验主键、partition与向量，拒绝非NULL hidden distance/k，OR REPLACE先删除现有身份后插入；随后写索引、auxiliary、metadata。

#### Scenario: Vector insert type and conflict behavior boundary
- **WHEN** 后置metadata类型验证失败
- **THEN** 实测失败后同连接仍能看到新rowid/vector，不保证语句原子回滚；FLOAT metadata不接受INTEGER，auxiliary INSERT允许NULL或精确类型。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vec0Update_Insert`。

### Requirement: Vector updates and null sentinel
UPDATE SHALL 拒绝partition修改及TEXT主键变化，支持flat/rescore向量更新，拒绝DiskANN/IVF非NULL向量更新；NULL向量作为跳过标记。

#### Scenario: Vector updates and null sentinel boundary
- **WHEN** 将向量设NULL、TEXT auxiliary改INTEGER或BOOLEAN写4294967296
- **THEN** 向量保留原值，auxiliary UPDATE不检查声明类型，大整数BOOLEAN经32位转换可接受并存0；这些边界不视为严格类型保证。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vec0Update_Update`。

### Requirement: Vector deletion and empty chunk reclamation
DELETE SHALL 删除身份与auxiliary、清有效位/rowid/向量/metadata及长文本，调用各索引清理并回收全空chunk。

#### Scenario: Vector deletion and empty chunk reclamation boundary
- **WHEN** 普通chunk中最后一行被删除
- **THEN** 对应chunks、flat/rescore/metadata chunk行移除并失效latest缓存；不承诺故障中途清理原子性或损坏状态的完整检测。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vec0Update_Delete`。

### Requirement: Vector table lifecycle and rename
vec0 SHALL 在Sync释放缓存statement，在Disconnect释放实例；Rename重命名各现有shadow表并更新缓存，Destroy按索引删除shadow表。

#### Scenario: Vector table lifecycle and rename boundary
- **WHEN** 事务调用Begin/Commit/Rollback
- **THEN** 这些回调本身为空OK，不证明完整撤销；ShadowName静态登记core/metadata，未覆盖所有vector/index后缀。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vec0Rename`。

### Requirement: Rescore storage and quantization
rescore SHALL 为float列保存量化chunk和原float KV，支持bit或int8；插入/更新同步两份，删除及chunk回收清理对应数据。

#### Scenario: Rescore storage and quantization boundary
- **WHEN** 零向量采用bit量化
- **THEN** rescore使用>=0得到FF，公开binary函数得到00；int8为unit步长夹取，不同于IVF乘127。

证据：`third_party/sqlite-vec/sqlite-vec-rescore.c` — `rescore_on_insert`。

### Requirement: Rescore candidate reranking and filter gap
rescore SHALL 先量化扫描最多min(k*oversample,4096)候选，再读取原float按配置metric重排取k；支持rowid IN。

#### Scenario: Rescore candidate reranking and filter gap boundary
- **WHEN** 提供distance范围或降低oversample
- **THEN** 当前distance条件被planner omit但未执行，可能返回不满足WHERE的结果；重排仅覆盖候选，不保证全集精确top-k。

证据：`third_party/sqlite-vec/sqlite-vec-rescore.c` — `rescore_knn`。

### Requirement: Rescore runtime oversample command
oversample=命令 SHALL 按atoi读取正整数并修改所有rescore列的内存搜索override。

#### Scenario: Rescore runtime oversample command boundary
- **WHEN** 传129、1trailing或0
- **THEN** 前两者接受，0拒绝；不同于schema上限128，无rescore列也可返回OK，设置不持久化。

证据：`third_party/sqlite-vec/sqlite-vec-rescore.c` — `rescore_handle_command`。

### Requirement: DiskANN node graph and beam search
DiskANN SHALL 保存原向量与有效位/邻居ID/量化邻居blob，从_info medoid进行有界候选搜索，结合量化邻居探索和原向量读取确认。

#### Scenario: DiskANN node graph and beam search boundary
- **WHEN** 近似距离比重算距离小
- **THEN** 重复候选仅接受更小距离，故confirmed可能保留近似值；[3]*8到零实测返回8而实际L2约8.485。node_read严格校验blob尺寸，其他路径不等同完整损坏库保护。

证据：`third_party/sqlite-vec/sqlite-vec-diskann.c` — `diskann_search`。

### Requirement: DiskANN insertion pruning and buffering
DiskANN SHALL 先存原向量，按threshold直接建图或buffer累计后flush；选邻居用alpha prune，反向边满时量化最差替换。

#### Scenario: DiskANN insertion pruning and buffering boundary
- **WHEN** buffer尚未达到阈值或删除medoid
- **THEN** buffer仍参与KNN完整距离合并；删除medoid选择第一个其他原向量为入口。flush/反向边部分错误处理不保证全部成功或回滚。

证据：`third_party/sqlite-vec/sqlite-vec-diskann.c` — `diskann_insert`。

### Requirement: DiskANN deletion and runtime settings
DiskANN DELETE SHALL 清buffer或修复图邻居、删节点/vector、调整medoid并scrub残留引用；search_list_size系列命令设置首个DiskANN列。

#### Scenario: DiskANN deletion and runtime settings boundary
- **WHEN** 使用正整数运行时参数或损坏节点
- **THEN** 参数atoi且无独立上限、不持久化；节点读取失败在删除入口可能按不存在返回成功，不能宣称所有失败被传播。

证据：`third_party/sqlite-vec/sqlite-vec-diskann.c` — `diskann_delete`。

### Requirement: DiskANN query constraint differences
DiskANN KNN SHALL 走独立类型维数验证路径，k<=0空结果；其当前实现未使用公共4096上限及planner下推的distance/rowid IN。

#### Scenario: DiskANN query constraint differences boundary
- **WHEN** k=4097或多值IN(2,3)、distance>100
- **THEN** 4097接受，IN可能返回1，distance可返回小于100值；这些已复现差异属于现状限制，不能视为SQL过滤正确支持。

证据：`third_party/sqlite-vec/sqlite-vec.c` — `vec0Filter_knn_diskann`。

### Requirement: Experimental IVF configuration and cells
显式启用IVF宏后 SHALL 提供nlist/nprobe、none/int8/binary量化及oversample；存centroid、64槽cell、rowid_map及量化时原float KV，未训练数据归-1。

#### Scenario: Experimental IVF configuration and cells boundary
- **WHEN** nlist小于默认nprobe或quantizer none且oversample>1
- **THEN** 默认nprobe夹到nlist，显式超限拒绝；none加oversample>1拒绝。int8距离固定L2，binary Hamming；实现按float32输入布局处理。

证据：`third_party/sqlite-vec/sqlite-vec-ivf.c` — `vec0_parse_ivf_options`。

### Requirement: Experimental IVF training and commands
IVF SHALL 提供compute-centroids、set-centroid、assign-vectors、clear-centroids与nprobe内存命令，默认作用第一个IVF列。

#### Scenario: Experimental IVF training and commands boundary
- **WHEN** compute-centroids带参数或量化manual操作
- **THEN** 参数以strstr/atoi提取而非JSON解析；训练尝试savepoint但多处返回码忽略；set/assign/clear存在量化布局与stride限制，不声明这些组合正确。

证据：`third_party/sqlite-vec/sqlite-vec-ivf.c` — `ivf_handle_command`。

### Requirement: Experimental IVF search and mutation limitations
IVF SHALL 扫未分配或最近nprobe中心的cells，以量化距离排序，量化且oversample>1时读取原float重排候选。

#### Scenario: Experimental IVF search and mutation limitations boundary
- **WHEN** 删早期槽再插新行或提供distance/rowid IN
- **THEN** slot=n_vectors且删除只减计数，已复现覆盖存活向量；过滤参数未进入IVF查询，WHERE可能失效；point返回cell字节，不能保证量化模式输出原float。

证据：`third_party/sqlite-vec/sqlite-vec-ivf.c` — `ivf_query_knn`。

### Requirement: IVF kmeans initialization and iteration
聚类 SHALL 使用xorshift32和kmeans++初值，seed0换42、k裁到N，以平方L2分配并求均值，无分配变化即停止；空cluster重置最远样本。

#### Scenario: IVF kmeans initialization and iteration boundary
- **WHEN** N/D/k非正或分配失败
- **THEN** 返回-1；默认调用迭代25次，训练距离不随列cosine/L1改变，后续IVF分配另按列metric。分配使用SQLite allocator。

证据：`third_party/sqlite-vec/sqlite-vec-ivf-kmeans.c` — `ivf_kmeans`。
