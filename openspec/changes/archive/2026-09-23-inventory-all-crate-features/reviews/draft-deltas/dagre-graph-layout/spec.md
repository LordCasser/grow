## ADDED Requirements

### Requirement: Dagre public graph model
GraphConfig、GraphNode与GraphEdge SHALL 暴露配置、几何、层级和dummy中间字段，默认node几何0、node/rank sep50及edge sep20。

#### Scenario: 实现条件
- **WHEN** 构造GraphEdge默认值
- **THEN** labeloffset为Some(0)，独立补缺省helper对None填10，不能将两者视为相同默认。

证据：`third_party/dagre_rust/src/lib.rs` — `GraphConfig`。

### Requirement: Dagre layout copy and output
layout SHALL 在新directed/multigraph/compound图完成pipeline，再回写node xy、compound尺寸、edge points/xy及graph尺寸。

#### Scenario: 实现条件
- **WHEN** 输入含内部字段
- **THEN** build实际clone全部label，非白名单复制；回写不包含rank/order等中间字段。

证据：`third_party/dagre_rust/src/layout/mod.rs` — `layout`。

### Requirement: Dagre pipeline preconditions
run_layout SHALL 依次执行标签留白、自环摘除、去环、nesting、rank、normalization、order、position及恢复。

#### Scenario: 实现条件
- **WHEN** 独立调用公开阶段helper
- **THEN** 依赖DAG、rank、完整metadata等前提，无统一Result、事务或输入资源限额。

证据：`third_party/dagre_rust/src/layout/mod.rs` — `run_layout`。

### Requirement: Dagre edge label spacing
make_space_for_edge_labels SHALL ranksep减半且minlen翻倍，非c标签依方向增加width或height的offset。

#### Scenario: 实现条件
- **WHEN** 标签双维度均正
- **THEN** proxy保存中间label rank；fixup仅edge.x非0时调整l/r坐标，x0标签不能推导相同调整。

证据：`third_party/dagre_rust/src/layout/mod.rs` — `make_space_for_edge_labels`。

### Requirement: Dagre label proxy restoration
inject_edge_label_proxies SHALL 对双正尺寸标签创建edge-proxy，保存端点中间rank。

#### Scenario: 实现条件
- **WHEN** 移除proxy
- **THEN** 将rank转回edge.label_rank后删除节点；rank min/max来自compound上下border。

证据：`third_party/dagre_rust/src/layout/mod.rs` — `inject_edge_label_proxies`。

### Requirement: Dagre DFS cycle breaking
acyclic run SHALL 默认及未知acyclicer使用递归DFS找回边，反转并保存原name，分配临时rev名称。

#### Scenario: 实现条件
- **WHEN** acyclicer为greedy
- **THEN** 仅向stdout输出greedy_fas，没有实现该算法；不得宣称支持greedy去环。

证据：`third_party/dagre_rust/src/layout/acyclic.rs` — `dfs_fas`。

### Requirement: Dagre cycle reversal restoration boundary
acyclic undo SHALL 为reversed边重新插入原方向及原name并清原向label标志。

#### Scenario: 实现条件
- **WHEN** 恢复反转边
- **THEN** 当前不删除临时反向边；顶层按原edge查询回写，不等同中间图拓扑完全恢复。

证据：`third_party/dagre_rust/src/layout/acyclic.rs` — `undo`。

### Requirement: Dagre self edge geometry
自环 SHALL 暂存于node、排序后插dummy并定位，再恢复原边与标签。

#### Scenario: 实现条件
- **WHEN** 恢复points
- **THEN** 当前生成6个中间点且首两点重复，之后统一补矩形相交端点。

证据：`third_party/dagre_rust/src/layout/mod.rs` — `position_self_edges`。

### Requirement: Dagre compound nesting constraints
nesting run SHALL 创建root和compound上下border，minlen乘层级因子2*height+1，用sum weights加1构造约束。

#### Scenario: 实现条件
- **WHEN** cleanup
- **THEN** 删除root和nesting边，保留rank factor；递归层级及计算依赖合法树结构。

证据：`third_party/dagre_rust/src/layout/nesting_graph.rs` — `run`。

### Requirement: Dagre ranker dispatch
rank SHALL 对None选network-simplex，对三个已知名称分别分派network-simplex、tight-tree和longest-path。

#### Scenario: 实现条件
- **WHEN** 未知Some ranker
- **THEN** 不执行rank算法，没有自动fallback。

证据：`third_party/dagre_rust/src/layout/rank/mod.rs` — `rank`。

### Requirement: Dagre longest path ranks
longest_path SHALL 从sources递归，以后继rank减round(minlen)最小值赋rank，sink为0。

#### Scenario: 实现条件
- **WHEN** 独立调用
- **THEN** 不normalize；slack也round但None minlen fallback10，与longest_path的0不同。

证据：`third_party/dagre_rust/src/layout/rank/util.rs` — `longest_path`。

### Requirement: Dagre feasible tight tree
feasible_tree SHALL 从首节点扩展零slack边，并按最小跨界slack平移树节点rank直至覆盖。

#### Scenario: 实现条件
- **WHEN** 图不连通
- **THEN** 无跨界边时循环可能不进展；要求非空connected DAG，不保证任意图终止。

证据：`third_party/dagre_rust/src/layout/rank/feasible_tree.rs` — `feasible_tree`。

### Requirement: Dagre network simplex iteration
network_simplex SHALL 简化多边后初始化longest path与tight tree，迭代负cut离开和最小slack进入边。

#### Scenario: 实现条件
- **WHEN** 没有进入候选
- **THEN** 停止；只回写rank，无迭代上限，update_ranks的minlen转i32截断区别初始round。

证据：`third_party/dagre_rust/src/layout/rank/network_simplex.rs` — `network_simplex`。

### Requirement: Dagre long edge dummy chains
normalize run SHALL 清edge points，将跨多rank边拆成无name短边，在label rank保存label尺寸。

#### Scenario: 实现条件
- **WHEN** undo
- **THEN** 沿首successor收集dummy点恢复原edge；依赖rank递增且链完整，不保证同rank/反向非法输入还原。

证据：`third_party/dagre_rust/src/layout/normalize/mod.rs` — `normalize_edge`。

### Requirement: Dagre dummy chain parents
parent_dummy_chains SHALL 以层级postorder区间和LCA路径，按dummy rank选择所属compound。

#### Scenario: 实现条件
- **WHEN** 缺successor
- **THEN** 停止该链；其它缺失metadata仍可能unwrap，不提供坏图自动修复。

证据：`third_party/dagre_rust/src/layout/parent_dummy_chains.rs` — `parent_dummy_chains`。

### Requirement: Dagre compound border segments
add_border_segments SHALL 对compound min..=max rank建立左右border节点、相邻rank边及parent。

#### Scenario: 实现条件
- **WHEN** 布局末尾
- **THEN** remove_border_nodes按边界位置计算compound尺寸，缺完整边界时跳过尺寸更新并删除border dummy。

证据：`third_party/dagre_rust/src/layout/add_border_segments.rs` — `add_border_segments`。

### Requirement: Dagre initial and sweep order
order SHALL 由leaf rank排序后的DFS初序出发，交替上下扫与左右bias，只有严格降低cross count才替换best。

#### Scenario: 实现条件
- **WHEN** 交叉数持平
- **THEN** 保留原best；连续无改善计数到4停止，不承诺全局最优布局。

证据：`third_party/dagre_rust/src/layout/order/mod.rs` — `order`。

### Requirement: Dagre layer graph construction
build_layer_graph SHALL 选择当前rank及覆盖rank的compound，保留parent并为顶层分配临时root，关系边统一指向movable且聚合weight。

#### Scenario: 实现条件
- **WHEN** 非movable邻居或compound label
- **THEN** 邻居未显式复制原order，compound改为仅border引用default label；依赖短边前提。

证据：`third_party/dagre_rust/src/layout/order/build_layer_graph.rs` — `build_layer_graph`。

### Requirement: Dagre weighted crossing count
cross_count SHALL 对相邻层累计weight交叉乘积，north出边按south位置排序并以累加树计数。

#### Scenario: 实现条件
- **WHEN** 出边目标不在south
- **THEN** 跳过，不统计任意跨层边；不修改输入图。

证据：`third_party/dagre_rust/src/layout/order/cross_count.rs` — `two_layer_cross_count`。

### Requirement: Dagre weighted barycenters
barycenter SHALL 按入边weight和源order计算重心，无入边时返回None。

#### Scenario: 实现条件
- **WHEN** 有入边但总weight0
- **THEN** 仍计算除法可得NaN，不能声明所有返回值有限。

证据：`third_party/dagre_rust/src/layout/order/barycenter.rs` — `barycenter`。

### Requirement: Dagre constraint resolution
resolve_conflicts SHALL 按约束拓扑LIFO处理，对缺重心或违反顺序的条目合并节点与权重。

#### Scenario: 实现条件
- **WHEN** 约束有环或合并总weight0
- **THEN** 环中未进入source的条目不输出，合并可0除；前提不成立时不是无损排序。

证据：`third_party/dagre_rust/src/layout/order/resolve_conflicts.rs` — `resolve_conflicts`。

### Requirement: Dagre subgraph ordering
sort_subgraph SHALL 递归合并子图重心、解约束、展开成员并将左右border夹在两端。

#### Scenario: 实现条件
- **WHEN** 重心相同或None
- **THEN** sort按bias原index决定tie并插回无重心项；沿parent新增相邻不同子图约束。

证据：`third_party/dagre_rust/src/layout/order/sort_subgraph.rs` — `sort_subgraph`。

### Requirement: Dagre coordinate direction transforms
coordinate_system SHALL 对lr/rl预交换宽高，bt/rl反y，lr/rl最终交换xy及恢复宽高。

#### Scenario: 实现条件
- **WHEN** rankdir大小写或未知值
- **THEN** 只匹配精确小写字符串；独立helper的None会unwrap，顶层补默认tb。

证据：`third_party/dagre_rust/src/layout/coordinate_system.rs` — `undo`。

### Requirement: Dagre vertical coordinates
position SHALL 在非compound图按层赋Y，层高为节点height转i32后的最大值，层间累加ranksep。

#### Scenario: 实现条件
- **WHEN** 节点高度含小数
- **THEN** 小数截断；只对position_x返回节点回写xy。

证据：`third_party/dagre_rust/src/layout/position/mod.rs` — `position_y`。

### Requirement: Dagre BK conflicts and alignment
position_x SHALL 检测内段及border冲突，计算ul/ur/dl/dr四组，以无冲突中位邻居形成root block。

#### Scenario: 实现条件
- **WHEN** 空layering
- **THEN** 直接返回空map；type1跳空层，type2只扫真实相邻层，两者不可混称同一覆盖。

证据：`third_party/dagre_rust/src/layout/position/bk.rs` — `position_x`。

### Requirement: Dagre horizontal block compaction
horizontal_compaction SHALL 构造相邻不同root的最大sep约束，两次扫描先最小坐标再去空隙，并按root回填。

#### Scenario: 实现条件
- **WHEN** dummy与普通节点相邻
- **THEN** sep使用edge/node各半间隔、半宽及labelpos方向修正，指定border type第二遍不移动。

证据：`third_party/dagre_rust/src/layout/position/bk.rs` — `horizontal_compaction`。

### Requirement: Dagre alignment balancing
balance SHALL 默认取四组坐标排序后的中间两值平均，显式align选择对应组。

#### Scenario: 实现条件
- **WHEN** 显式align未知
- **THEN** 节点坐标回填0；选择最窄组后平移过程依赖非空有限值，公开helper不全面防NaN。

证据：`third_party/dagre_rust/src/layout/position/bk.rs` — `balance`。

### Requirement: Dagre translation and rectangle endpoints
translate_graph SHALL 按节点与双正尺寸edge标签bounds加margin平移，assign_node_intersects补矩形边界交点。

#### Scenario: 实现条件
- **WHEN** 仅polyline超出或空图
- **THEN** polyline不参与bounds，空图可能非有限尺寸；shape字段不改变矩形相交算法。

证据：`third_party/dagre_rust/src/layout/mod.rs` — `translate_graph`。

### Requirement: Dagre unique dummy identifiers
unique_id SHALL 使用全局AtomicUsize Relaxed，add_dummy_node遇graph已有ID重试。

#### Scenario: 实现条件
- **WHEN** 整数耗尽
- **THEN** fetch_add及后续加1不提供无限单调/唯一保证；不等同随机ID。

证据：`third_party/dagre_rust/src/layout/util.rs` — `unique_id`。

### Requirement: Dagre graph simplification helpers
simplify SHALL 聚合同端点weight sum及minlen max，as_non_compound复制leaf与所有边，transfer复制leaf/edge labels。

#### Scenario: 实现条件
- **WHEN** 调用simplify_ref
- **THEN** 仅补None weight/minlen，不聚合；不应按注释将其当simplify等价物。

证据：`third_party/dagre_rust/src/layout/util.rs` — `simplify`。

### Requirement: Dagre rank matrix and normalization
build_layer_matrix SHALL 按rank分组并排序order，normalize_ranks减最小rank，remove_empty_ranks按factor压缩指定空层。

#### Scenario: 实现条件
- **WHEN** 重复order、负rank或巨大跨度
- **THEN** 同key可覆盖、负rank索引可panic、大跨度分配不受限；独立调用依赖合法rank。

证据：`third_party/dagre_rust/src/layout/util.rs` — `build_layer_matrix`。

### Requirement: Dagre utility partition and geometry test
partition SHALL 按predicate稳定clone到两组，intersect_rect在中心重合时取右边中心。

#### Scenario: 实现条件
- **WHEN** 现有单测通过
- **THEN** 仅证明一个8x4矩形中心样本；manifest禁用doctest，不支持完整布局验证结论。

证据：`third_party/dagre_rust/src/layout/util.rs` — `intersect_rect_returns_boundary_point_for_center_point`。
