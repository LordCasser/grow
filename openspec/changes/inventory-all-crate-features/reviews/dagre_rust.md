# dagre_rust 逐包核查（进行中）

当前分支codex/openspec-sdd。已完整读取Cargo.toml、src/lib.rs、layout/mod.rs、acyclic.rs、coordinate_system.rs、normalize/mod.rs、rank/mod.rs。其余模块待读，保持pending，不以顶层pipeline代替内部算法覆盖。

manifest为vendored library-only 0.0.5，publish=false、doctest=false、精确依赖graphlib_rust=0.0.2与ordered_hashmap=0.0.3。GraphNode公开几何/层级/内部dummy字段；GraphEdge/GraphConfig公开配置。默认edge labeloffset=Some(0)，补缺省helper对None填10，两者不同。

layout创建独立directed/multigraph/compound图，实际clone全部label字段，非注释所称白名单复制；补默认并复制parent/edge，部分graphlib写入错误忽略。run_layout按标签留白、自环摘除、去环、nesting、rank、代理、空rank清理、normalize/dummy、order、坐标变换/position、还原及相交点顺序执行。公开helper依赖阶段不变量，普遍unwrap，没有统一Result或资源限制。update仅回写node xy、compound尺寸、edge points/xy与graph尺寸，不复制内部rank等。

标签留白ranksep减半/minlen翻倍；非c按tb/bt增加width，其它方向height。双正尺寸label才建proxy；fixup用edge.x!=0判断，真实x0标签不调整。translate只把有双正尺寸的edge标签纳入bounds，polyline不影响extremes；空图初始min infinity可能得到非有限尺寸，需后续核对调用方而非推导任意输入保证。边端点按矩形相交，未按shape字段选形状。

自环先保存node.self_edges，后插dummy定位并恢复6个中间点，其中前两点相同；最后统一端点相交。compound尺寸取border位置，缺metadata时跳过；最终删除border dummy。

acyclic默认/未知值走DFS recursion找回边并反向改名。greedy分支仅println("greedy_fas")且无算法，不得记录为已支持greedy；与manifest“不含I/O”概括不一致（存在stdout）。undo为reversed边添加原方向/原name，却未删除反向边；目前只记录源码事实，不修运行时。

coordinate_system仅精确小写lr/rl交换宽高，bt/rl反y，lr/rl交换xy并恢复宽高；rankdir None在独立调用时unwrap，顶层已填默认。ranker None为network-simplex；三个已知字符串分别分派，未知Some不执行任何ranker，不能按注释声称fallback。

normalize先清所有edge.points，跨rank长边拆为无name短边，label rank处带尺寸。undo逐dummy首successor收集points并恢复原edge对象；依赖rank递增/DAG/完整链，非法同rank或反向边不保证可还原。后续需读rank/order/position/util/nesting等内部实现与唯一现有单测。

本轮只读源码并记录事实，没有构建缓存。

## rank、nesting与util完整读取

新增完整读取rank/util.rs、feasible_tree.rs、network_simplex.rs、nesting_graph.rs、add_border_segments.rs、parent_dummy_chains.rs与layout/util.rs。尚余order和position模块。

longest_path从sources递归，sink rank0、取后继rank减round(minlen)的最小值，不自行normalize。slack同样round，None minlen独立调用fallback10，与longest_path fallback0不同。feasible_tree从首节点构造无向tight tree，按最小跨界slack平移rank；断开图没有跨界edge时while不进展，空图插空字符串树节点。仅在已满足connected DAG等前提下使用，不承诺任意graph终止。

network_simplex先simplify多重边、longest_path、feasible_tree，再low/lim与cutvalue，选首负cut离开和最小slack进入，交换后重算树信息与rank。没有进入edge则break，无迭代预算；只回写rank。update_ranks将minlen直接as i32截断，区别初始round；不据函数注释声称独立调用结果rank从0。DFS使用递归，未设深度上限。

nesting创建root、按层级深度将minlen乘2*height+1，sum(weights)+1作为约束权重；leaf root edge权重0，compound top/bottom border及nesting edge约束内部位置。cleanup删除root和标记nesting边，保留node_rank_factor供空rank处理。border segments对min..=max逐层增左右节点并相邻rank串联、设置parent。parent_dummy_chains通过层级postorder/LCA路径、rank区间选择dummy parent，沿首successor前进；缺successor break但不提供坏图全面恢复。

unique_id为全局AtomicUsize Relaxed fetch_add+1，跨线程无数据竞争，但整数耗尽后不保证永远单调/唯一，debug加1还可panic；add_dummy_node按graph已有ID重试。simplify聚合同端点weight sum与minlen max（初始minlen1），不保留所有edge元数据；simplify_ref仅补None字段，不聚合，注释“same logic”不能照搬。as_non_compound复制leaf后复制全部edges；transfer复制source leaf与edges，不转graph config。

intersect_rect中心重合时返回右边中心，其余按矩形半宽高射线交点；唯一单测只验证中心重合8x4矩形，未证明任意零尺寸/非有限值安全。build_layer_matrix以rank分配并按order排序，相同rank/order覆盖同key，负rank转usize后unwrap；不是任意节点无损分层。normalize_ranks缺rank按0参与求min但只改Some。remove_empty_ranks按min offset建完整跨度数组，factor<=0返回，压缩非factor倍数空层；大rank跨度资源无限制。partition按predicate稳定分两组clone，add_border_node可选usize rank转i32。

本轮未执行构建，继续保留pending。

## order与position完整读取

order全部9模块及position/mod.rs、bk.rs完整读取，24份Rust源码已全部读完。init_order按leaf rank稳定排序后DFS后继形成初序；order以初序cross_count作为best，只严格改善才替换，交替上下扫和左右bias，连续无改善计数至4停止（改善后本轮仍加1），不是全局最优保证。每层构建compound layer graph，边按关系重定向到movable节点并聚合weight；邻居由set_edge加入，未显式复制非movable邻居的原order标签，不能照搬注释称完整共享视图。compound层节点被替换为仅border引用的default label。

barycenter按入边weight乘order求和，零总weight仍返回Some(sum/0)；resolve_conflicts按约束图拓扑LIFO、逆序ins合并，缺barycenter或逆序即合并，合并也可能0/0；约束有环的非source条目不会输出，独立公开调用不保证无损。sort按barycenter及bias原index排序，None条目按原index插回；NaN比较落入tie bias。sort_subgraph递归扩展子图、夹左右border、按border首pred补重心；add_subgraph_constraints沿parent建立首次相邻不同兄弟约束。

cross_count对相邻两层，过滤目标不在south的边，按每north目标排序，用累加树计算weight乘交叉weight；无图修改，不算非邻层边。层级数组及整数上限未统一检查。

position先noncompound图，Y按每层节点height转i32后取max再转f32（小数截断），中心加半高、累加ranksep。只对position_x返回节点回写xy。

BK检测type1（跳过空层后相邻非空层）与type2（真实相邻非空层）冲突，用字典序规范化node pair。type2尾scan位于每个south节点迭代内，存在重复扫描。position_x以OrderedHashMap extend合并两类冲突（同外层key的内表合并语义需依赖容器，不假定逐pair union）。vertical_alignment择可用中位邻居形成root，返回align.keys向量而非align映射，compaction仅用这些key回填root x。

horizontal_compaction按相邻不同root构block graph，sep取两半宽、node/edge各半间隔并按labelpos/反向修正；同block边保留最大sep。两次迭代先最小坐标再尽量右移，特定边界type不移动。position_x计算ul/ur/dl/dr，右向取负，选几何宽最小对齐，再以坐标min/max平移；默认取四值中间两项均值，显式未知align回填0。独立辅助函数的空map/NaN仍可能unwrap panic；顶层position_x全空层直接空map。

本包动态测试只有矩形中心相交单测；即便通过也不能证明完整布局算法正确或所有输入安全。接下来运行现有测试并登记规范，当前仍pending。

验证：cargo test --locked -p dagre_rust退出0，1 passed、0 failed/ignored，manifest禁用doctest。日志/tmp/grow-dagre-tests.log；关闭incremental及DEV/TEST_DEBUG，随后立即cargo clean本工作树独立target，删除25文件1.3MiB。测试范围仅上述几何helper，不能支持完整布局验证声明。规范整合未完成，保持pending。

## 规范登记

32项要求已写入dagre-graph-layout并纳入feature-map、包清单及证据哈希。未修改运行时代码。
