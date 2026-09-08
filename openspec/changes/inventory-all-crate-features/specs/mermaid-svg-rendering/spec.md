## ADDED Requirements

### Requirement: Mermaid render dispatch
render_mermaid_to_svg SHALL 在剥离frontmatter后，按首个非空非注释行的精确类型token分派图表实现。

#### Scenario: 实现边界
- **WHEN** 类型不是专用分派项
- **THEN** 交给普通flowchart parser判断，不统一提前返回unsupported。

证据：`third_party/mermaid-to-svg/src/lib.rs` — `render_mermaid_to_svg`。

### Requirement: Mermaid theme precedence
渲染 SHALL 优先采用调用方显式theme，其次frontmatter配置theme，最后默认主题。

#### Scenario: 实现边界
- **WHEN** 显式theme与frontmatter同时存在
- **THEN** 显式theme获胜；不同图表renderer实际消费的颜色字段须按各自契约解释。

证据：`third_party/mermaid-to-svg/src/lib.rs` — `render_mermaid_to_svg`。

### Requirement: Mermaid language fence recognition
is_mermaid_diagram SHALL 转小写后只接受mermaid或mermaid加ASCII空格前缀。

#### Scenario: 实现边界
- **WHEN** 语言标记带前导空格或仅tab后缀
- **THEN** 不trim且不把任意空白视为空格前缀。

证据：`third_party/mermaid-to-svg/src/lib.rs` — `is_mermaid_diagram`。

### Requirement: Mermaid frontmatter boundary
frontmatter SHALL 只在起始空行后完整---行和后续完整---行之间识别YAML块。

#### Scenario: 实现边界
- **WHEN** 没有闭合分隔符
- **THEN** 返回借用原文；闭合时返回剥离头块的正文。

证据：`third_party/mermaid-to-svg/src/config.rs` — `frontmatter_bounds`。

### Requirement: Mermaid invalid YAML stripping
parse_mermaid_frontmatter SHALL 对闭合但非法YAML仍剥离头块并使用默认metadata及config。

#### Scenario: 实现边界
- **WHEN** YAML解析失败
- **THEN** 不抛出渲染错误，也不把坏YAML保留为图表正文。

证据：`third_party/mermaid-to-svg/src/config.rs` — `parse_mermaid_frontmatter`。

### Requirement: Mermaid configuration coercion
配置解析 SHALL 按精确键读取，字符串接受string/number/bool，布尔接受bool或精确true/false字符串，u32接受可转换unsigned或字符串。

#### Scenario: 实现边界
- **WHEN** 未知键或无效类型
- **THEN** 忽略该字段，不视为完整schema验证。

证据：`third_party/mermaid-to-svg/src/config.rs` — `parse_render_config`。

### Requirement: Mermaid font size normalization
fontSize SHALL 只接受有限正数，可去除小写px后缀。

#### Scenario: 实现边界
- **WHEN** 零、负数、非有限值或不识别单位
- **THEN** 回退默认字号；不将任意CSS长度交给字体引擎。

证据：`third_party/mermaid-to-svg/src/config.rs` — `parse_font_size`。

### Requirement: Mermaid stored configuration limits
配置 SHALL 保留layout/look/securityLevel及部分flowchart选项，但当前渲染不据这些字段切换布局或执行安全策略。

#### Scenario: 实现边界
- **WHEN** 设置securityLevel或htmlLabels
- **THEN** 不能推导输入HTML会执行或已经由该设置净化。

证据：`third_party/mermaid-to-svg/src/config.rs` — `RenderConfig`。

### Requirement: Mermaid state configuration boundary
stateDiagram SHALL 转为FlowchartGraph后使用默认布局与renderer配置。

#### Scenario: 实现边界
- **WHEN** state输入含flowchart间距或字体配置
- **THEN** 这些配置不沿该入口传入；theme仍按入口选择。

证据：`third_party/mermaid-to-svg/src/lib.rs` — `render_mermaid_to_svg`。

### Requirement: Mermaid disabled experimental port
实验port SHALL 在当前公开渲染入口恒禁用。

#### Scenario: 实现边界
- **WHEN** 设置MERMAID_TO_SVG_USE_PORT环境变量
- **THEN** is_enabled仍为false，不读取环境切换引擎；内部移植代码存在不代表可选运行模式。

证据：`third_party/mermaid-to-svg/src/mermaid_port/mod.rs` — `is_enabled`。

### Requirement: Mermaid Unicode width estimation
文本宽度 SHALL 用Unicode显示宽度单位乘近似字符宽计算，不调用真实字体度量。

#### Scenario: 实现边界
- **WHEN** CJK、字体切换或多字素输入
- **THEN** 保留显示宽度估算语义，不保证等于浏览器bbox。

证据：`third_party/mermaid-to-svg/src/text_wrap.rs` — `display_width_units`。

### Requirement: Mermaid word wrapping
wrap_text_lines SHALL 按LF分原始行、trim并按whitespace拆词，以ASCII空格重建，贪心装入宽度。

#### Scenario: 实现边界
- **WHEN** 空文本或空原始行
- **THEN** 空文本无行，空行保留空词行；非有限max_width按无限宽处理。

证据：`third_party/mermaid-to-svg/src/text_wrap.rs` — `wrap_text_lines`。

### Requirement: Mermaid long token split
不可分词token SHALL 在不超过宽度上限五倍时保持整体，超过时先取至少一个grapheme保证推进，再择ASCII标识符分隔符断开。

#### Scenario: 实现边界
- **WHEN** token超过cap且含_-./
- **THEN** 优先最后可用分隔符并保留分隔符；该二次切分不保证组合字符簇始终完整。

证据：`third_party/mermaid-to-svg/src/text_wrap.rs` — `split_token_at_cap`。

### Requirement: Mermaid wrapped height
换行度量 SHALL 以默认16字号缩放首行24高度，后续按字号乘1.1递增。

#### Scenario: 实现边界
- **WHEN** 传入无效字号
- **THEN** 按默认16处理，不将NaN字号传入高度计算。

证据：`third_party/mermaid-to-svg/src/text_wrap.rs` — `wrapped_text_height_with_font_size`。

### Requirement: Mermaid flowchart declaration
普通parser SHALL 识别graph/flowchart声明及TB/TD/BT/LR/RL方向，按行解析正文。

#### Scenario: 实现边界
- **WHEN** 同一行包含分号或header尾部语句
- **THEN** 不执行完整分号grammar，header尾部不作为后续语句。

证据：`third_party/mermaid-to-svg/src/parser.rs` — `parse_graph_declaration`。

### Requirement: Mermaid flowchart statement termination
语句收集 SHALL 跳过空行和%%注释，并遇end结束当前语句列表。

#### Scenario: 实现边界
- **WHEN** 顶层出现end或子图缺end
- **THEN** 顶层余文不解析；子图可读至EOF，不保证结构错误均拒绝。

证据：`third_party/mermaid-to-svg/src/parser.rs` — `parse_statements`。

### Requirement: Mermaid flowchart edge chains
边链 SHALL 先收集节点再追加相邻边，扫描时保护括号深度和双引号中的箭头。

#### Scenario: 实现边界
- **WHEN** 单引号、转义引号或非对称形状含箭头
- **THEN** 不提供完整quote-aware grammar；缺末端可停止而非报错。

证据：`third_party/mermaid-to-svg/src/parser.rs` — `parse_edge_chain`。

### Requirement: Mermaid flowchart label normalization
节点label SHALL 去外层配对引号、执行有限实体单轮解码，并将支持的BR及literal反斜线n转LF。

#### Scenario: 实现边界
- **WHEN** 嵌套HTML实体或任意HTML标签
- **THEN** 不执行通用HTML或Markdown解析。

证据：`third_party/mermaid-to-svg/src/parser.rs` — `normalize_label`。

### Requirement: Mermaid flowchart node shapes
节点语法 SHALL 按固定匹配顺序识别circle/stadium/cylinder/subroutine/hexagon/rectangle/rounded/diamond/asymmetric。

#### Scenario: 实现边界
- **WHEN** 多数形状缺ID
- **THEN** 从label中保留alphanumeric生成ID，可能空或碰撞；asymmetric无该fallback。

证据：`third_party/mermaid-to-svg/src/parser.rs` — `try_parse_node`。

### Requirement: Mermaid flowchart subgraph identifiers
subgraph SHALL 接受id[title]、单ID及多词标题自动subGraphN。

#### Scenario: 实现边界
- **WHEN** 自动ID或显式ID重复
- **THEN** 不执行唯一性校验；内部direction不形成独立方向设置。

证据：`third_party/mermaid-to-svg/src/parser.rs` — `parse_subgraph`。

### Requirement: Mermaid flowchart style parsing
style SHALL 按target后的逗号和首冒号收集property。

#### Scenario: 实现边界
- **WHEN** property缺冒号或包含任意颜色字符串
- **THEN** 缺冒号项忽略，颜色未验证；真正生效字段由布局消费决定。

证据：`third_party/mermaid-to-svg/src/parser.rs` — `parse_style`。

### Requirement: Mermaid node declaration replacement
布局收集 SHALL 保留首次节点order，有显式label的重复声明更新shape/label/尺寸。

#### Scenario: 实现边界
- **WHEN** 后续声明label None
- **THEN** 保留原节点，连shape也不覆盖。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `add_node`。

### Requirement: Mermaid subgraph ownership
布局 SHALL 在节点首次被子图认领时保存直接owner，不因后续引用迁移。

#### Scenario: 实现边界
- **WHEN** 子图在同名普通节点之后声明
- **THEN** 当前只识别已经收集的子图ID，可能保留同名普通节点；不保证名称空间隔离。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `collect_nodes_and_edges`。

### Requirement: Mermaid effective node styles
默认布局 SHALL 以最后style statement整体替换同节点properties，并取列表首fill/stroke。

#### Scenario: 实现边界
- **WHEN** 提供color、stroke-width或同列表重复fill
- **THEN** 其它字段不生效，重复fill取首项，不能按完整CSS解释。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `get_node_colors`。

### Requirement: Mermaid Dagre graph and duplicate edges
默认布局 SHALL 创建directed/multigraph/compound图，节点按首次order设置，边使用无name键。

#### Scenario: 实现边界
- **WHEN** 多条边映射相同端点
- **THEN** 底层几何由最后边覆盖，输出原关系仍可多份复用该几何。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `build_dagre_graph`。

### Requirement: Mermaid cycle edge detection
布局 SHALL 递归DFS识别回边并从Dagre建图排除，后续按几何方向判定路由。

#### Scenario: 实现边界
- **WHEN** 多根图或深图
- **THEN** roots未统一排序且无递归深度上限，不承诺所有输入布局完全确定或有界。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `detect_back_edges`。

### Requirement: Mermaid cluster endpoint anchors
cluster边 SHALL 以直辖成员首无内部入边/末无内部出边作为Dagre目标/源anchor。

#### Scenario: 实现边界
- **WHEN** 没有直辖成员或内部有环
- **THEN** 无成员退原cluster ID，有环退首/末成员；不是全子树入口出口搜索。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `dagre_edge_endpoint`。

### Requirement: Mermaid nested subgraph bounds
子图框 SHALL 自底向上合并直辖节点及已padding子图框，每级增加8和标题高度。

#### Scenario: 实现边界
- **WHEN** 子图无节点且无可见子框
- **THEN** 不输出该框；标题高度至少24但不根据标题宽扩框。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `compute_subgraph_bounds`。

### Requirement: Mermaid connected subgraph centering
有直接跨子图节点边的组 SHALL 尝试把各子图直辖节点跨轴均值移至组均值。

#### Scenario: 实现边界
- **WHEN** 拟议移动导致组内非祖孙框重叠
- **THEN** 整组跳过；居中步骤只更新节点positions。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `center_nodes_in_subgraphs`。

### Requirement: Mermaid subgraph overlap resolution
布局 SHALL 对非祖孙交叠子图沿跨轴正移中心较大的一侧，距离为重叠加25。

#### Scenario: 实现边界
- **WHEN** 子图数平方轮后仍有重叠
- **THEN** 不报错且不保证完全消除；仅全在移动子树内的普通节点边同步Dagre点。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `resolve_subgraph_overlaps`。

### Requirement: Mermaid state rank adjustment
检测到特殊state形状时布局 SHALL 使用longest-path ranker并按无回边rank映射已有y层。

#### Scenario: 实现边界
- **WHEN** 只有普通状态节点或y层不足
- **THEN** 前者不触发state模式，后者跳过snap；该修正固定y轴。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `snap_state_ranks`。

### Requirement: Mermaid state terminal alignment
state终端 SHALL 在非首层唯一、无forward outgoing且至少两条前层入边时对齐前驱最大x。

#### Scenario: 实现边界
- **WHEN** 重复入边
- **THEN** 也计入数量，不能宣称按唯一前驱平均居中。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `align_state_terminal_singletons`。

### Requirement: Mermaid shape measurement
节点 SHALL 以共享wrap度量和各shape padding公式确定尺寸，state特殊形状使用专门常量。

#### Scenario: 实现边界
- **WHEN** 多行circle标签
- **THEN** 直径只按textwidth加padding，未取textheight最大值，不保证全部文字装入。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `measure_node`。

### Requirement: Mermaid collapsed internal ranks
子图内部边主轴距离小于1时 SHALL 按直辖内部关系拓扑rank重新分层。

#### Scenario: 实现边界
- **WHEN** 反向图或配置rankSpacing
- **THEN** 修正仍沿主轴正向并使用固定50，未按BT/RL反转或采用配置间距。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `fix_subgraph_internal_ranks`。

### Requirement: Mermaid obstacle routing limits
fallback路由 SHALL 在非端点节点扩10框中选首阻挡者向近侧外30绕行。

#### Scenario: 实现边界
- **WHEN** 多个障碍或子图框
- **THEN** 不重验全部障碍且不把子图框作为障碍，不保证路径无碰撞。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `compute_edge_points_with_obstacles`。

### Requirement: Mermaid back edge routes
几何回边 SHALL 使用专用U形折点，垂直选左右外30，水平聚合跨度内节点后择上下近侧。

#### Scenario: 实现边界
- **WHEN** 垂直回边中间有障碍
- **THEN** 不扫描该障碍；平滑U点公式未专门翻转BT/RL。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `compute_back_edge_points`。

### Requirement: Mermaid self loop routes
自环 SHALL 使用五个点，size在40到60之间，垂直向右、水平向上。

#### Scenario: 实现边界
- **WHEN** 水平自环越过零y
- **THEN** 最终左上shift未按裸edgepoints最小值计算，不能保证无标签自环不裁切。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `compute_self_loop_points`。

### Requirement: Mermaid aligned edge straightening
跨轴差小于15的非回边 SHALL 尝试均值轴直线，并检查非端点节点扩5框。

#### Scenario: 实现边界
- **WHEN** 候选穿过其它节点
- **THEN** 保留Dagre点；该检查仅候选首末线段，不是通用曲线碰撞保证。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `straighten_if_aligned`。

### Requirement: Mermaid cluster interior route trimming
cluster端点路径 SHALL 删除多余内部折点，保留一个过渡点及至少两点。

#### Scenario: 实现边界
- **WHEN** 点恰在边界或整条路径都在内部
- **THEN** 边界视为外；无外点时保持原路径。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `trim_cluster_interior_points`。

### Requirement: Mermaid shape clipping approximation
裁剪 SHALL 对circle/state使用圆方程、diamond使用菱形方程，其余形状按矩形。

#### Scenario: 实现边界
- **WHEN** hexagon/cylinder/stadium/asymmetric端点
- **THEN** 使用矩形近似，不保证命中真实轮廓。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `connection_point_on_node`。

### Requirement: Mermaid layout final bounds
布局 SHALL 用节点/子图/标签确定左上平移，再将边点及标签右下范围计入总尺寸。

#### Scenario: 实现边界
- **WHEN** renderer随后移动边标签避让
- **THEN** 不回写此布局尺寸，最终标签可能超viewBox。

证据：`third_party/mermaid-to-svg/src/layout.rs` — `compute_with_dagre`。

### Requirement: Mermaid info fixed output
info SHALL 输出400x150固定图与常量v11.12.2。

#### Scenario: 实现边界
- **WHEN** header后有正文
- **THEN** 不解析正文，版本非运行时探测。

证据：`third_party/mermaid-to-svg/src/info_diagram.rs` — `render_info_diagram_to_svg`。

### Requirement: Mermaid pie input and slices
pie SHALL 按首冒号解析f64项，只绘原总量至少1%的扇区并降序排列。

#### Scenario: 实现边界
- **WHEN** 负数、NaN或过滤小项
- **THEN** 不逐项拒绝，过滤后不归一，单项100%无整圆特例。

证据：`third_party/mermaid-to-svg/src/pie_diagram.rs` — `render_pie_diagram_to_svg`。

### Requirement: Mermaid pie legend and theme
pie SHALL 保留原序全部图例，showData显示原数值，使用12色及固定白底黑字。

#### Scenario: 实现边界
- **WHEN** 重复label或大量图例
- **THEN** 按首同label查颜色，高450不扩，theme不生效。

证据：`third_party/mermaid-to-svg/src/pie_diagram.rs` — `render_pie_diagram_to_svg`。

### Requirement: Mermaid packet bit rows
packet SHALL 解析u32范围并要求后块紧接前块，按32bit分行重复标签。

#### Scenario: 实现边界
- **WHEN** 首起点>=32或巨大范围
- **THEN** 首起点未强制0，分片可下溢，无资源cap；固定黑灰且无wrap。

证据：`third_party/mermaid-to-svg/src/packet_diagram.rs` — `render_packet_diagram_to_svg`。

### Requirement: Mermaid timeline section grouping
timeline SHALL 按section名称分组，冒号空格行追加到最近任务。

#### Scenario: 实现边界
- **WHEN** section前已有无section任务
- **THEN** 存在section时这些任务不绘，同名section合并；theme未应用。

证据：`third_party/mermaid-to-svg/src/timeline_diagram.rs` — `render_timeline_diagram_to_svg`。

### Requirement: Mermaid timeline sizing
timeline SHALL 使用固定190宽及字节长度估高。

#### Scenario: 实现边界
- **WHEN** 长文本或组间切换
- **THEN** 实际文字不换行，组间额外推进，不保证真实文字bounds。

证据：`third_party/mermaid-to-svg/src/timeline_diagram.rs` — `render_timeline_diagram_to_svg`。

### Requirement: Mermaid journey scores and actors
journey SHALL 解析i32 score、逗号actor并按连续section绘任务。

#### Scenario: 实现边界
- **WHEN** score超1..5或重复actor
- **THEN** 不拒绝，面部可超画布，任务内actor重复绘。

证据：`third_party/mermaid-to-svg/src/journey_diagram.rs` — `render_journey_diagram_to_svg`。

### Requirement: Mermaid journey label fallback
journey SHALL 输出转义XHTML及SVG文字fallback。

#### Scenario: 实现边界
- **WHEN** 长文本或大量actor
- **THEN** 固定框尺寸不扩，theme未应用。

证据：`third_party/mermaid-to-svg/src/journey_diagram.rs` — `render_journey_diagram_to_svg`。

### Requirement: Mermaid quadrant coordinates
quadrant SHALL 要求至少一点并把普通坐标clamp0..1，在500x500绘四象限。

#### Scenario: 实现边界
- **WHEN** NaN或其它象限编号
- **THEN** NaN未拒绝，只绘1..4；background以#0/#1粗判dark。

证据：`third_party/mermaid-to-svg/src/quadrant_diagram.rs` — `render_quadrant_chart_to_svg`。

### Requirement: Mermaid radar axis and curves
radar SHALL 要求至少一axis，按最大值至少1归一并绘五圈网格。

#### Scenario: 实现边界
- **WHEN** 曲线维度不匹配或多series
- **THEN** 不匹配曲线不绘但参与max，CSS只有series0，非有限值未清洗。

证据：`third_party/mermaid-to-svg/src/radar_diagram.rs` — `render_radar_diagram_to_svg`。

### Requirement: Mermaid sankey input and cycles
sankey SHALL 按裸逗号三列解析link，按前驱depth最多迭代2N次分层。

#### Scenario: 实现边界
- **WHEN** CSV引号或有环输入
- **THEN** 不提供CSV引号语义、不拒绝环，采用最后轮结果。

证据：`third_party/mermaid-to-svg/src/sankey_diagram.rs` — `render_sankey_diagram_to_svg`。

### Requirement: Mermaid sankey scale and palette
sankey SHALL 用入出总量最大值作node值，以层可用高度求ky。

#### Scenario: 实现边界
- **WHEN** 负数、拥挤或十色以上
- **THEN** 可负尺寸，十色后均退首色，theme仅背景生效。

证据：`third_party/mermaid-to-svg/src/sankey_diagram.rs` — `render_sankey_diagram_to_svg`。

### Requirement: Mermaid mindmap indentation
mindmap SHALL 按空格/tab各1缩进建树，生成内部n序号ID，跳过::装饰。

#### Scenario: 实现边界
- **WHEN** 多个同层根
- **THEN** 后根挂原根下，不产生多根错误。

证据：`third_party/mermaid-to-svg/src/mindmap_diagram.rs` — `render_mindmap_to_svg`。

### Requirement: Mermaid mindmap radial layout
mindmap SHALL 按叶子数分径向角度，子树继承8色，估Unicode宽度。

#### Scenario: 实现边界
- **WHEN** 仅根或长label
- **THEN** 仅根未归一会裁切负半边，无wrap/避碰，theme未用。

证据：`third_party/mermaid-to-svg/src/mindmap_diagram.rs` — `render_mindmap_to_svg`。

### Requirement: Mermaid gantt dependencies
gantt SHALL 按id/start/duration前三项建任务，after只引用先前ID。

#### Scenario: 实现边界
- **WHEN** 重复ID或dateFormat
- **THEN** 后ID覆盖依赖表但保留任务，dateFormat忽略，不支持状态参数语义。

证据：`third_party/mermaid-to-svg/src/gantt_diagram.rs` — `parse_gantt_diagram`。

### Requirement: Mermaid gantt calendar limits
gantt SHALL 接受整数日期与i32 d/D/w/W时长。

#### Scenario: 实现边界
- **WHEN** 无效月日或负时长
- **THEN** 不校验日历有效性或非负性，运算未统一防溢出。

证据：`third_party/mermaid-to-svg/src/gantt_diagram.rs` — `parse_duration_days`。

### Requirement: Mermaid gantt render scale
gantt SHALL 固定784宽并逐日生成tick，任务保留输入序。

#### Scenario: 实现边界
- **WHEN** 非连续同section或大跨度
- **THEN** 背景按类别汇总可能错配，tick不抽稀，theme未用。

证据：`third_party/mermaid-to-svg/src/gantt_diagram.rs` — `render_gantt_diagram_to_svg`。

### Requirement: Mermaid kanban grammar
kanban SHALL 把未缩进行作列、任意首whitespace行作task。

#### Scenario: 实现边界
- **WHEN** task先于列或只有header
- **THEN** 前者报错，后者空board合法，不解析id[label]。

证据：`third_party/mermaid-to-svg/src/kanban_diagram.rs` — `parse_kanban`。

### Requirement: Mermaid kanban metadata
kanban SHALL 从单行@{...} YAML读取label/assigned/priority/ticket标量。

#### Scenario: 实现边界
- **WHEN** ticket或未知字段
- **THEN** ticket仅存储不绘链接，未知字段忽略，非法YAML报错。

证据：`third_party/mermaid-to-svg/src/kanban_diagram.rs` — `apply_shape_data`。

### Requirement: Mermaid kanban card rendering
kanban SHALL 用原生SVG文字绘固定宽卡，有assigned高56否则44。

#### Scenario: 实现边界
- **WHEN** 第11列之后或长label
- **THEN** CSS只有section0..10，文本无wrap；priority按精确值映射标线。

证据：`third_party/mermaid-to-svg/src/kanban_diagram.rs` — `render_kanban_diagram_to_svg`。

### Requirement: Mermaid block grammar
block SHALL 按每行节点或首-->关系解析，columns支持整数和auto。

#### Scenario: 实现边界
- **WHEN** 分组、style、space指令
- **THEN** 均跳过，未实现嵌套、样式和占位；columns0有节点时可除零。

证据：`third_party/mermaid-to-svg/src/block_diagram.rs` — `parse_block_beta`。

### Requirement: Mermaid block node labels
block SHALL 以首[拆ID和标签，闭]可缺，去配对单双引号。

#### Scenario: 实现边界
- **WHEN** 重复ID
- **THEN** 首次顺序保留，仅新label!=id才覆盖，不支持同一行多节点或边链。

证据：`third_party/mermaid-to-svg/src/block_diagram.rs` — `parse_block_node`。

### Requirement: Mermaid block grid geometry
block SHALL 将节点尺寸归一最大值并按声明序排矩形网格。

#### Scenario: 实现边界
- **WHEN** 空图或长文字
- **THEN** 空图bounds可Infinity，文字不wrap；边统一箭头，矩形裁剪。

证据：`third_party/mermaid-to-svg/src/block_diagram.rs` — `render_block_diagram_to_svg`。

### Requirement: Mermaid block attribute escaping
block SHALL 转义节点id/label和edge id中的端点。

#### Scenario: 实现边界
- **WHEN** 端点含引号等特殊字符
- **THEN** 边class的ls/le来自未转义小写ID，不能宣称全部SVG属性已转义。

证据：`third_party/mermaid-to-svg/src/block_diagram.rs` — `render_block_diagram_to_svg`。

### Requirement: Mermaid gitgraph branch operations
gitGraph SHALL 默认main，branch继承当前head但不切换，checkout切到指定分支。

#### Scenario: 实现边界
- **WHEN** checkout未知分支
- **THEN** 创建空head分支，不继承旧head；重复branch不操作。

证据：`third_party/mermaid-to-svg/src/gitgraph_diagram.rs` — `parse_gitgraph`。

### Requirement: Mermaid gitgraph commit identity
commit SHALL 生成内部序号并取当前head为父，忽略附加参数。

#### Scenario: 实现边界
- **WHEN** 用户id/tag/type
- **THEN** 不形成提交元数据，展示标签来自序号确定性hash，不是真实Git hash。

证据：`third_party/mermaid-to-svg/src/gitgraph_diagram.rs` — `parse_gitgraph`。

### Requirement: Mermaid gitgraph merge semantics
merge SHALL 总创建Merge节点并收集当前及目标head。

#### Scenario: 实现边界
- **WHEN** 未知目标或自合并
- **THEN** 不报错，可能零/单父或重复父；无fast-forward判断。

证据：`third_party/mermaid-to-svg/src/gitgraph_diagram.rs` — `parse_gitgraph`。

### Requirement: Mermaid gitgraph visual layout
gitGraph SHALL 按分支创建序y间隔90、提交全局序x步50排列，merge双圆无标签。

#### Scenario: 实现边界
- **WHEN** 分支超过8个
- **THEN** CSS只定义0..7颜色，序号不取模；方向header尾文未生效。

证据：`third_party/mermaid-to-svg/src/gitgraph_diagram.rs` — `render_gitgraph_diagram_to_svg`。

### Requirement: Mermaid XY supported plots
xychart SHALL 只把line声明作为数值系列，要求至少一个非空系列。

#### Scenario: 实现边界
- **WHEN** bar或其它未知语句
- **THEN** 忽略；混合空系列保留索引但不绘制。

证据：`third_party/mermaid-to-svg/src/xychart_diagram.rs` — `parse_xychart`。

### Requirement: Mermaid XY category parsing
XY分类 SHALL 对单双引号内逗号作保护并去空类别。

#### Scenario: 实现边界
- **WHEN** 转义引号或未闭引号
- **THEN** 不执行完整escape或闭合验证。

证据：`third_party/mermaid-to-svg/src/xychart_diagram.rs` — `parse_category_list`。

### Requirement: Mermaid XY numeric domains
XY数值轴 SHALL 支持可选标题和min-->max，y未显式范围时从数据自动取值。

#### Scenario: 实现边界
- **WHEN** 反向、相等或非有限范围
- **THEN** 不拒绝；常值auto范围扩±1，非有限极值退0..0但数据不清洗。

证据：`third_party/mermaid-to-svg/src/xychart_diagram.rs` — `parse_labeled_range`。

### Requirement: Mermaid XY series alignment
XY SHALL 所有series共用最长系列长度作为x索引域，分类band取类别与点数较大者。

#### Scenario: 实现边界
- **WHEN** 数值单点或缺x-axis
- **THEN** 单点位于左端，缺轴默认0..0，不能按用户逐点x值理解。

证据：`third_party/mermaid-to-svg/src/xychart_diagram.rs` — `layout_x_axis`。

### Requirement: Mermaid XY plot presentation
XY SHALL 在700x500绘M/L折线，Tableau10循环并用theme文字色绘轴。

#### Scenario: 实现边界
- **WHEN** 长分类、超y范围或单点
- **THEN** 字体仅缩至8，无clipPath，超域可能越界，单点仅M不可见。

证据：`third_party/mermaid-to-svg/src/xychart_diagram.rs` — `render_xychart_diagram_to_svg`。

### Requirement: Mermaid XY malformed number lists
XY SHALL 以首[和末]截取逗号数值列表，空项忽略。

#### Scenario: 实现边界
- **WHEN** 括号逆序或NaN
- **THEN** 未检查end>start可panic，未检查有限性。

证据：`third_party/mermaid-to-svg/src/xychart_diagram.rs` — `parse_bracketed_number_list`。

### Requirement: Mermaid state declarations
state SHALL 接受两种state header并转固定TB图，state首token作ID。

#### Scenario: 实现边界
- **WHEN** quoted alias或复合状态
- **THEN** 无完整alias/层级解析；choice/fork/join仅按声明包含标记决定shape。

证据：`third_party/mermaid-to-svg/src/state_diagram.rs` — `parse_state_diagram`。

### Requirement: Mermaid state transitions
state SHALL 按首-->及右侧首冒号构造迁移，自动创建缺失节点。

#### Scenario: 实现边界
- **WHEN** [*]或空端点
- **THEN** 起终分别共用__start/__end且可能与用户ID碰撞，空端点未拒绝。

证据：`third_party/mermaid-to-svg/src/state_diagram.rs` — `parse_state_diagram`。

### Requirement: Mermaid requirement node kinds
requirement SHALL 支持六种需求类型和element，kind大小写不敏感，name仅首token。

#### Scenario: 实现边界
- **WHEN** 重复声明或缺闭括号
- **THEN** 同名覆盖，EOF可接受未闭块，不实现带空格quoted名称。

证据：`third_party/mermaid-to-svg/src/requirement_diagram.rs` — `try_parse_requirement_or_element`。

### Requirement: Mermaid requirement properties
需求 SHALL 读取id/text/risk/verifyMethod，element读取type/docref，按固定字段顺序绘制。

#### Scenario: 实现边界
- **WHEN** 未知属性或risk值
- **THEN** 未知属性忽略，已知risk及verify规范首字母，其它值照存不拒绝。

证据：`third_party/mermaid-to-svg/src/requirement_diagram.rs` — `try_parse_requirement_or_element`。

### Requirement: Mermaid requirement relationships
requirement SHALL 支持->和<-关系语句，保存任意关系type。

#### Scenario: 实现边界
- **WHEN** 未声明端点
- **THEN** Dagre可有隐式节点但无metrics故不绘，非端点完整性验证。

证据：`third_party/mermaid-to-svg/src/requirement_diagram.rs` — `try_parse_relation`。

### Requirement: Mermaid requirement layout
requirement SHALL 使用四方向Dagre及按文本估算的需求框，边和标签纳入bounds。

#### Scenario: 实现边界
- **WHEN** 同端点多关系
- **THEN** 底层无name键覆盖，原关系复用最后几何/尺寸，非独立多边布局。

证据：`third_party/mermaid-to-svg/src/requirement_diagram.rs` — `compute_layout`。

### Requirement: Mermaid requirement contains marker
contains精确小写 SHALL 绘实线无末箭头，其它关系虚线开放箭头。

#### Scenario: 实现边界
- **WHEN** 期待包含圆加号
- **THEN** containsStart只定义未引用，不显示该marker。

证据：`third_party/mermaid-to-svg/src/requirement_diagram.rs` — `render_svg`。

### Requirement: Mermaid ER entity attributes
ER SHALL 将实体块属性首token作type，余文整体作name，引用自动补空实体。

#### Scenario: 实现边界
- **WHEN** PK/FK/comment或缺闭括号
- **THEN** 无独立键字段语义，EOF仍接受；重复实体块覆盖前块。

证据：`third_party/mermaid-to-svg/src/er_diagram.rs` — `parse_er_diagram`。

### Requirement: Mermaid ER cardinality
ER SHALL 接受四种基数符号，--识别关系实线、..非识别关系虚线。

#### Scenario: 实现边界
- **WHEN** 左右基数
- **THEN** card_a标记start、card_b标记end，不交换端点。

证据：`third_party/mermaid-to-svg/src/er_diagram.rs` — `parse_cardinality`。

### Requirement: Mermaid ER layout and labels
ER SHALL 使用固定TB Dagre，按type/name两列估宽，无wrap。

#### Scenario: 实现边界
- **WHEN** 多条同端点关系
- **THEN** 无独立edge name，复用最后几何和label尺寸；角色文本不去引号。

证据：`third_party/mermaid-to-svg/src/er_diagram.rs` — `compute_layout`。

### Requirement: Mermaid ER dark stripes
ER SHALL 在暗背景下由背景混白8%/16%生成条纹，亮背景用白及nodeFill混白25%。

#### Scenario: 实现边界
- **WHEN** 非六位hex或非ASCII颜色
- **THEN** 辅助按字节slice，不支持一般CSS颜色且可能UTF8边界panic；非统一颜色净化。

证据：`third_party/mermaid-to-svg/src/er_diagram.rs` — `render_entity_node`。

### Requirement: Mermaid class declarations
class SHALL 支持class块及Name:member，含(成员归method，其余attribute。

#### Scenario: 实现边界
- **WHEN** 重复块或stereotype
- **THEN** 成员追加，块stereotype覆盖甚至清空；单行stereotype只当attribute。

证据：`third_party/mermaid-to-svg/src/class_diagram.rs` — `parse_class_diagram`。

### Requirement: Mermaid class relation operators
class SHALL 将18种关系符号归为八种RelationType。

#### Scenario: 实现边界
- **WHEN** 反向符号或点线composition
- **THEN** 反向未交换marker端，点线composition/aggregation归实线，不完整保留原语义。

证据：`third_party/mermaid-to-svg/src/class_diagram.rs` — `classify_relationship`。

### Requirement: Mermaid class cardinality labels
class SHALL 识别操作符旁独立双引号token的基数。

#### Scenario: 实现边界
- **WHEN** 两端基数及关系label同时存在
- **THEN** 按源基数、label、目标基数拼成中央文字，不在端点分别定位。

证据：`third_party/mermaid-to-svg/src/class_diagram.rs` — `parse_class_diagram`。

### Requirement: Mermaid class box sizing
class SHALL 按标题、stereotype、属性及方法分区估算尺寸并总绘两条分隔线。

#### Scenario: 实现边界
- **WHEN** 空成员区或长文字
- **THEN** 保留空区gap，不换行，stereotype显示guillemet。

证据：`third_party/mermaid-to-svg/src/class_diagram.rs` — `compute_class_box_size`。

### Requirement: Mermaid class layout bounds
class SHALL 按ID顺序建Dagre节点，并仅以节点bounds设至少100的画布。

#### Scenario: 实现边界
- **WHEN** 边label或marker外伸
- **THEN** 不计这些bounds、不额外normalize；同端点边仍复用底层几何。

证据：`third_party/mermaid-to-svg/src/class_diagram.rs` — `compute_layout`。

### Requirement: Mermaid class edge clipping
class SHALL 按矩形裁剪两端，再沿首末向量为始marker退17或末marker退6。

#### Scenario: 实现边界
- **WHEN** 短边或曲折路径
- **THEN** 无长度上限且非沿局部段偏移，不能保证箭头精确贴边。

证据：`third_party/mermaid-to-svg/src/class_diagram.rs` — `clip_and_offset_edge`。

### Requirement: Mermaid class theme boundary
class SHALL 对空fill/stroke/edge/text回默认颜色，其他值直接插SVG。

#### Scenario: 实现边界
- **WHEN** 设置background或多图同页
- **THEN** background未使用，固定marker ID及全局svg CSS未隔离。

证据：`third_party/mermaid-to-svg/src/class_diagram.rs` — `render_svg`。

### Requirement: Mermaid C4 declarations
C4 SHALL 支持五种header及Person/System/Container/Component、Db/Queue和外部变体。

#### Scenario: 实现边界
- **WHEN** 缺参数或重复alias
- **THEN** 缺参补空，重复shape保留，不校验唯一性；不同header共享grammar。

证据：`third_party/mermaid-to-svg/src/c4_diagram.rs` — `parse_c4_diagram`。

### Requirement: Mermaid C4 function arguments
C4 SHALL 按双引号与括号深度分逗号参数并去双引号。

#### Scenario: 实现边界
- **WHEN** 转义引号或多行调用
- **THEN** 不支持完整escape或多行调用，非法/未知行静默跳过。

证据：`third_party/mermaid-to-svg/src/c4_diagram.rs` — `split_c4_args`。

### Requirement: Mermaid C4 boundaries
C4 SHALL 记录boundary栈并按声明序布局有直属shape的边界。

#### Scenario: 实现边界
- **WHEN** 嵌套或只含子boundary
- **THEN** 忽略parent层级几何，外框可不绘；不是完整嵌套容器。

证据：`third_party/mermaid-to-svg/src/c4_diagram.rs` — `render_c4_diagram_to_svg`。

### Requirement: Mermaid C4 relations
C4 SHALL 查首同alias shape作为端点，所有关系绘单向末箭头。

#### Scenario: 实现边界
- **WHEN** BiRel或方向变体
- **THEN** rel_type不参与渲染，双向和方向提示未实现；缺端点跳过。

证据：`third_party/mermaid-to-svg/src/c4_diagram.rs` — `render_rels`。

### Requirement: Mermaid C4 dynamic numbering
C4Dynamic SHALL 为关系标签按原索引加一编号，首关系直线其余quadratic。

#### Scenario: 实现边界
- **WHEN** 首关系缺端点
- **THEN** 后续仍按原索引曲线/编号，允许编号空缺。

证据：`third_party/mermaid-to-svg/src/c4_diagram.rs` — `render_rels`。

### Requirement: Mermaid C4 presentation
C4 SHALL 使用固定类型配色、白字、Db/Queue轮廓及Person内嵌PNG。

#### Scenario: 实现边界
- **WHEN** 长文本或theme节点颜色
- **THEN** 字节长度估宽且不wrap，节点不使用theme颜色；无外部图片请求。

证据：`third_party/mermaid-to-svg/src/c4_diagram.rs` — `render_c4_shape`。

### Requirement: Mermaid sequence participants
sequence SHALL 支持participant/actor及精确 as 分割的别名声明。

#### Scenario: 实现边界
- **WHEN** 两侧均无空白
- **THEN** 较短UTF8字节者作ID，同长选左，不固定语法左侧为ID。

证据：`third_party/mermaid-to-svg/src/sequence_diagram.rs` — `parse_participant_declaration`。

### Requirement: Mermaid sequence alias merging
sequence SHALL 按ID/label/alias相交合并首匹配参与者，保留首次ID和顺序。

#### Scenario: 实现边界
- **WHEN** 展示名称相同但ID不同
- **THEN** 可能合并；仅旧label==id时升级label，未知引用自动创建。

证据：`third_party/mermaid-to-svg/src/sequence_diagram.rs` — `register_participant`。

### Requirement: Mermaid sequence lifecycle limits
create SHALL 注册参与者，destroy只解析引用，box只跳过分组头尾。

#### Scenario: 实现边界
- **WHEN** 期待出生销毁或box背景
- **THEN** 生命线仍全高，actor同矩形，未实现生命周期与盒背景。

证据：`third_party/mermaid-to-svg/src/sequence_diagram.rs` — `parse_sequence_diagram`。

### Requirement: Mermaid sequence message arrows
sequence SHALL 识别十种箭头，目标前+激活目标、前-停用源。

#### Scenario: 实现边界
- **WHEN** 头含多个箭头或空端点
- **THEN** 按固定优先序首匹配，非quote-aware且空引用未拒绝。

证据：`third_party/mermaid-to-svg/src/sequence_diagram.rs` — `parse_message_line`。

### Requirement: Mermaid sequence autonumber
autonumber SHALL 支持开关、u64起点和步长，消息编号用saturating_add。

#### Scenario: 实现边界
- **WHEN** 无效数字、0步长或溢出
- **THEN** 无效保留旧值，0或饱和可重复编号；note不编号。

证据：`third_party/mermaid-to-svg/src/sequence_diagram.rs` — `AutoNumber`。

### Requirement: Mermaid sequence note grammar
note SHALL 支持over一人/两人及left/right of，要求首冒号分正文。

#### Scenario: 实现边界
- **WHEN** 多逗号或引号冒号
- **THEN** 不提供完整quote-aware参数，title/note仅解#59;，message不解。

证据：`third_party/mermaid-to-svg/src/sequence_diagram.rs` — `parse_note_line`。

### Requirement: Mermaid sequence fragments
sequence SHALL 支持alt/loop/opt/par/critical/break/rect及else/and/option。

#### Scenario: 实现边界
- **WHEN** 缺end或游离分隔
- **THEN** 未闭自动闭合，游离else增高无框，未验证分隔类型。

证据：`third_party/mermaid-to-svg/src/sequence_diagram.rs` — `parse_fragment_line`。

### Requirement: Mermaid sequence activations
activation SHALL 按参与者后进先出匹配，嵌套偏移4，宽8最小高6。

#### Scenario: 实现边界
- **WHEN** 未闭或额外deactivate
- **THEN** 未闭延伸到底，额外deactivate忽略；事件不占行取上次行中心。

证据：`third_party/mermaid-to-svg/src/sequence_diagram.rs` — `compute_activation_layouts`。

### Requirement: Mermaid sequence fragment geometry
片段 SHALL 覆盖全部参与者跨度，每层向内18。

#### Scenario: 实现边界
- **WHEN** 深嵌套或rect颜色
- **THEN** 宽度未clamp可负；rect无背景颜色语义，label不显示。

证据：`third_party/mermaid-to-svg/src/sequence_diagram.rs` — `render_fragment`。

### Requirement: Mermaid sequence sizing
sequence SHALL 共用最大参与者框宽，消息估宽扩间距，精确<br/>拆参与者行。

#### Scenario: 实现边界
- **WHEN** 超过两行、长自消息或note
- **THEN** header仍44，自消息文本宽不计，note固定26高无wrap。

证据：`third_party/mermaid-to-svg/src/sequence_diagram.rs` — `render_sequence_diagram_to_svg`。

### Requirement: Mermaid SVG paint order
通用SVG SHALL 按子图背景、边、ID排序节点、子图标题、边标签绘制。

#### Scenario: 实现边界
- **WHEN** 布局尺寸
- **THEN** 直接使用整数画布尺寸，不依据最终文字重新布局。

证据：`third_party/mermaid-to-svg/src/svg_renderer.rs` — `render`。

### Requirement: Mermaid SVG shape rendering
renderer SHALL 为全部12种NodeShape绘制对应几何。

#### Scenario: 实现边界
- **WHEN** start/end/fork
- **THEN** 不绘label且不使用普通节点fill/stroke覆盖；asymmetric实际为左突五边形。

证据：`third_party/mermaid-to-svg/src/svg_renderer.rs` — `render_node`。

### Requirement: Mermaid SVG text options
renderer SHALL 使用fontFamily、字号与wrap宽度输出中心对齐tspan。

#### Scenario: 实现边界
- **WHEN** 非整数字号
- **THEN** 度量保留小数但输出字号取整数；文本及fontFamily转义，颜色直接插入。

证据：`third_party/mermaid-to-svg/src/svg_renderer.rs` — `render_text_lines`。

### Requirement: Mermaid SVG edge styles
边 SHALL 按六种style控制箭头、3 3点线及1/3.5线宽。

#### Scenario: 实现边界
- **WHEN** 末段长度<=箭头偏移
- **THEN** 不缩短末段，不保证箭头尖端始终贴边。

证据：`third_party/mermaid-to-svg/src/svg_renderer.rs` — `render_edge_line`。

### Requirement: Mermaid SVG curve mode
curve SHALL 仅对linear使用M/L，其它名称用直角修正后basis。

#### Scenario: 实现边界
- **WHEN** 未知curve名称
- **THEN** 不报错，按basis处理，不提供任意Mermaid曲线算法。

证据：`third_party/mermaid-to-svg/src/svg_renderer.rs` — `edge_path_d`。

### Requirement: Mermaid SVG label placement
标签 SHALL 优先双正坐标label_pos，否则取修正折线长度中点。

#### Scenario: 实现边界
- **WHEN** 标签相互重叠
- **THEN** 最多10轮两两沿最小重叠轴分离，不检查节点/边或扩画布，不保证收敛。

证据：`third_party/mermaid-to-svg/src/svg_renderer.rs` — `render_edge_labels`。

### Requirement: Mermaid SVG label background
边标签 SHALL 使用固定浅灰rgba(.8)背景和主题文字色。

#### Scenario: 实现边界
- **WHEN** 暗色theme
- **THEN** 背景仍固定浅灰，不能声称完全主题化。

证据：`third_party/mermaid-to-svg/src/svg_renderer.rs` — `render_edge_labels`。

### Requirement: Mermaid port database boundary
内部port SHALL 后序登记子图，style创建节点并追加properties。

#### Scenario: 实现边界
- **WHEN** 与默认layout比较
- **THEN** owner首次且fill/stroke最终后值获胜，不能视为默认入口同义行为。

证据：`third_party/mermaid-to-svg/src/mermaid_port/flow_db.rs` — `collect_statements`。

### Requirement: Mermaid port flow data order
内部FlowData SHALL 先倒序groups后vertex_order节点并复制边。

#### Scenario: 实现边界
- **WHEN** group与vertex同ID
- **THEN** 不在转换阶段校验唯一性。

证据：`third_party/mermaid-to-svg/src/mermaid_port/flow_data.rs` — `get_data`。

### Requirement: Mermaid port cluster extraction
内部port SHALL 为外连cluster选择anchor并抽取无外连子图。

#### Scenario: 实现边界
- **WHEN** 深嵌套
- **THEN** extractor深度>10停止，但前置递归无相同cap，不等于整体资源限制。

证据：`third_party/mermaid-to-svg/src/mermaid_port/cluster_adjust.rs` — `adjust_clusters_and_edges`。

### Requirement: Mermaid port recursive layout
内部port SHALL 先布局抽取子图再以其尺寸布局父图并平移合并。

#### Scenario: 实现边界
- **WHEN** 关系端点被anchor改写
- **THEN** edge metadata仍原端点key，提取时不匹配可丢边；默认公开入口禁用该路径。

证据：`third_party/mermaid-to-svg/src/mermaid_port/dagre_layout_port.rs` — `layout_recursive`。

### Requirement: Mermaid port options and dimensions
内部port SHALL 向抽取子图传播node/rank spacing，并使用普通flowchart尺寸公式。

#### Scenario: 实现边界
- **WHEN** state特殊图或子图标题
- **THEN** 无默认layout的state专用度量/标题留白和后处理，不计为公开可选模式。

证据：`third_party/mermaid-to-svg/src/mermaid_port/dagre_layout_port.rs` — `apply_options_to_extracted`。

### Requirement: Mermaid theme preset parsing
主题预设 SHALL 精确接受 default、base、dark、forest、neutral，默认主题与 base 均采用 light 的七项颜色。

#### Scenario: 实现边界
- **WHEN** 输入 Dark 或未知预设
- **THEN** parse 返回 None，不做大小写转换或推导主题。

证据：`third_party/mermaid-to-svg/src/theme.rs` — `pub fn parse`。

### Requirement: Mermaid theme variable aliases
主题变量 SHALL 将 primaryColor/mainBkg、primaryBorderColor/nodeBorder、primaryTextColor/nodeTextColor/textColor、lineColor/defaultLinkColor 分别映射到节点填充、边框、文本和边颜色；background、clusterBkg、clusterBorder 映射到背景和子图颜色。

#### Scenario: 实现边界
- **WHEN** 依次应用多个同字段别名或未知键
- **THEN** 后一次调用覆盖同字段；未知键返回 false，不修改其它字段。

证据：`third_party/mermaid-to-svg/src/theme.rs` — `apply_mermaid_alias`。

### Requirement: Mermaid theme sparse overrides
主题变量 SHALL 仅覆盖值为 Some 的七个字段；is_empty 仅在全部字段为 None 时成立。

#### Scenario: 实现边界
- **WHEN** 设置空字符串或未经验证的颜色文本
- **THEN** 仍作为原字符串覆盖，不验证 CSS 格式或自动计算对比度。

证据：`third_party/mermaid-to-svg/src/theme.rs` — `pub fn apply_to`。

### Requirement: Mermaid public error surface
MermaidError SHALL 提供带行号和消息的 ParseError，以及携带字符串的 InvalidDirection、InvalidNodeShape、DotGenerationError、RenderError、UnsupportedDiagramType。

#### Scenario: 实现边界
- **WHEN** 调用公开 render 入口
- **THEN** 返回 Result<String, MermaidError>；枚举变体存在不保证所有输入错误都被捕获或每个变体都有可达返回路径。

证据：`third_party/mermaid-to-svg/src/error.rs` — `pub enum MermaidError`。

### Requirement: Mermaid vendored library boundary
本地包 SHALL 作为 publish=false 的 SVG 生成库构建，不提供已移除的上游 CLI；doctest=false，无 Cargo feature 开关。

#### Scenario: 实现边界
- **WHEN** 执行包测试或消费输出
- **THEN** 包测试不执行已移除的上游 fixture/snapshot 或 doctest；SVG 栅格化由调用方负责。

证据：`third_party/mermaid-to-svg/Cargo.toml` — `doctest = false`。

