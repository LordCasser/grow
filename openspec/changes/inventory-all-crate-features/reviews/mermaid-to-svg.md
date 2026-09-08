# mermaid-to-svg 逐包核查

全部35份 Rust 文件及 Cargo.toml 已读取，126项契约已登记到 mermaid-svg-rendering。79项单元测试通过；测试后清理独立 target。下文按读取批次保留核查记录，其中阶段性的“待核对”由后续记录覆盖。

manifest为library-only、publish=false、doctest=false，移除上游CLI和外部snapshot assets，保留本地in-source测试。vendoring说明仅作线索，每个声明仍需源码复核；不能用说明替代实际算法或测试审阅。

render入口先解析frontmatter，theme优先显式参数、其次配置theme、最后default。首个非空且非%%开头行的首whitespace token按大小写精确分派：er/class/mindmap/state(两种)/pie/gantt/requirement/info/packet/block/radar/sankey/sequence/gitGraph/timeline/journey/kanban/quadrant/xychart及5个C4头。state转FlowchartGraph但使用默认layout/render，不传flowchart配置。flowchart/graph才使用config布局与renderer；其它未分派输入继续普通parser，不等同入口立即UnsupportedDiagramType。

is_mermaid_diagram对lang转小写后仅等于mermaid或以mermaid加ASCII空格开头，不trim，不能推导任意空白后缀/前缀接受。strip_frontmatter委托parser，未匹配借用原文，匹配则owned正文。

frontmatter仅跳过起始空行后认trim为---的完整行，找到下一---为结束；未闭合保留原文。YAML非法仍剥离block并返回Some(default metadata)+default config，不抛error。metadata仅title；读取config精确键，string接受string/number/bool，bool接受bool或精确true/false字符串，u32接受可转换unsigned或parse字符串，非法/未知字段静默忽略。font_size只去小写px并检查有限正数。

layout/look/securityLevel、htmlLabels/diagramPadding/useMaxWidth/defaultRenderer被保存但标记未渲染，不能把securityLevel写成安全策略已执行。themeVariables逐alias收集；仅theme或变量非空时生成theme。具体alias及渲染使用待theme/layout模块核对。

mermaid_port is_enabled恒false，没有读取MERMAID_TO_SVG_USE_PORT；公开render路径不可达实验port，但保留的模块仍须遍历。MermaidError提供ParseError行号、direction/shape、DOT/render、unsupported类型变体；变体存在不证明实际每条路径都会返回。

lib测试已逐项读：全部类型基本smoke多数只检查SVG标签/文本，非完整语法或视觉验证；另有open label variants、括号内edge token、CJK宽度、class cardinality错误、ER暗色行、frontmatter precedence、sequence activation/extended grammar等具体断言。测试内容不能替代对应parser/renderer审阅。本轮未动态测试、未构建。

## theme、text_wrap、AST完整读取

theme.rs全部读取：preset精确default/base/dark/forest/neutral，Base等于light。主题只有background、node fill/stroke、text、edge、subgraph fill/stroke七项；别名primaryColor/mainBkg、primaryBorderColor/nodeBorder、primaryTextColor/nodeTextColor/textColor、lineColor/defaultLinkColor、clusterBkg、clusterBorder和background分别赋对应值。按调用顺序覆盖，不验证颜色格式、不自动衍生其它颜色；SVG实际插值安全仍需renderer核查，不能假设字符串经过CSS sanitization。

text_wrap.rs及7项in-source测试全部读取。字体无实际font测量，UnicodeWidthStr窄单位乘char_width；默认font16、lineheight1.1、wrap200、char8、textheight24。空字符串返回零行；按LF分段、trim每段，空段留一个空word；split_whitespace折叠内部空白，重建word间ASCII space。max_width非有限（包括NaN和负infinity）统一当正infinity，有限负数不纠正；char_width没有统一有效性验证。font_size无效才退16。

多word贪心按max width分行；单个token超过普通width但不超5倍cap整段保留，导致box可超wrapping_width。超cap先按grapheme前缀拆，至少一个字素保证进展，然后优先最后_/-/./处分割并把分隔符留首段。该二次ASCII边界选择只保证char boundary；若分隔符与后继combining字符组成一个grapheme，可能在字素内部拆开，不能照搬注释声称所有场景不丢/不拆字素。高度首行24按font比例、后续每行font*1.1；宽度不是glyph像素上界，测试只证明固定ASCII/CJK与分隔符样本。

ast.rs全部读取：FlowchartGraph四方向与Node/Edge/Subgraph/Style statement，12种NodeShape（含state相关），6种EdgeStyle（普通/dotted/thick，各arrow/line）。字段/enum存在不代表parser所有Mermaid语法均可达；后续逐parser核对。当前未运行测试、未产生target缓存。

## flowchart parser与info完整读取

parser.rs 659行完整读取。header要求小写graph/flowchart加ASCII space，direction大小写归一TD/TB/BT/LR/RL；同header行后续内容不作为statement。known但未分派的classDiagram-v2/flowchart-elk返回Unsupported；其它错误走ParseError。按行parse，无通用semicolon拆语句；空/%%整行跳过，顶层end也直接停止，余文不继续。未知非edge且无法parse node行静默忽略，subgraph缺end至EOF也接受。

find_edge_start按byte扫描括号深度与双引号，识别9种prefix；不验证括号类型配对、引号转义或单引号保护，Asymmetric >也不形成label保护。edge chain先collect nodes再追加edges；缺末目标直接break，起点parse失败可extract raw ID，不保证非法ID全部拒绝。pipe label未闭合时可退到无label同prefix继续，不统一报错；open label取最早closer、同位置最长优先，没有quote-aware closer解析。

形状识别按首find+末ends优先级：circle、stadium、cylinder、subroutine、hexagon、rectangle、rounded、diamond、asymmetric。多数空ID由label的Unicode alphanumeric生成，可为空/碰撞；Asymmetric不执行该fallback。bare ID仅alnum或underscore；未支持的语句可能被当ID或忽略。normalize_label trim、去配对单双外引号，解有限HTML实体（amp最后，只一轮），将literal反斜线n及精确大小写BR变体转LF，不是HTML/Markdown解析器。

subgraph显式id[title]或多word自动subGraphN，单token为id，无碰撞检测；内部direction语句未特殊处理。style按ASCII空格分target/rest，再按逗号/首冒号拆property，缺冒号项忽略，值未验证，实际可用property由renderer决定。

info_diagram.rs完整读取：只验证首有效token info，后续source不解析；输出固定400x150 viewBox、width100%、id my-svg，显示常量v11.12.2，不是本crate或运行时Mermaid版本探测。颜色直接插style，背景#ffffff变white、text#333333简写#333；不提供配置颜色sanitization。整体安全取决于调用方/后续SVG消费边界，后续renderer审阅继续核实。

## pie完整读取

pie_diagram.rs全部读取。header精确pie，任意后续token showData开启图例数值；title前缀覆盖非空标题。slice按首冒号拆分（引号内冒号不受保护），label仅去配对双引号，f64 parse不拒绝负数/NaN/infinity；只检查总和<=0和至少一项，NaN总和绕过比较但过滤后可能无扇区，不保证非法数值被拒绝。

渲染完全忽略传入theme，固定白底黑字、12色循环、450高/185半径。仅绘原总和占比>=1%的项并按值降序，不重新归一过滤后总量，可能留空角度；负值仍影响原总和。每扇区一个SVG arc，没有100%整圆特例。图例保留原顺序/所有项，以label找首颜色，重复label可能和扇区颜色不一致。legend宽按UTF-8 byte len乘10而非共享Unicode度量，高固定不随条目数增长；标题和图例文本执行五种XML转义。showData图例用原数值格式，扇区显示round后的整数百分比。源码事实已记录，未运行本包测试。

## packet完整读取

packet_diagram.rs全部读取。首有效token精确packet-beta，需至少一块；title后非空覆盖。range是u32单bit或start-end且end>=start；label去配对单双引号，裸空拒绝但quoted empty接受，不做反斜线转义。按输入顺序检查后项start=前end+1，不排序，首项不强制从0；last_bit+1普通加法未checked。

split_into_rows从row1起按32bit切分，部分边界计算saturating，但不保证整体合法：首start>=32时可先生成end<start片段，后续width的u32减法可能panic/错误巨大值；巨大range仍产生大量rows，不设资源cap。输出固定bit宽32、block高32、paddingX5、paddingY15，跨行重复label，始末bit标号（单bit只一个），title在底部且即使None也输出空text。

theme参数被忽略，固定黑字/黑边/灰fill，没有显式背景。图宽1026、高随row数量和title决定；label无换行/度量/裁剪，title和label做五种XML字符转义。仅记录源码行为与边界，不运行危险输入或修运行时；本包整体仍pending。

## timeline完整读取

timeline_diagram.rs完整读取。header精确timeline，正文跳空/%%/#行，title覆盖，section名称去重并保存当前组。冒号空格开头追加到最近task（不存在则丢弃）；其余首冒号拆period/event，裸行也是period，多个冒号不会分多个event，空period的未匹配行可退为bare task。无task由renderer拒绝。

存在任意section时只绘匹配section的task，先前section=None任务不绘；同名section复用同组。render_tasks已经推进master_x，外层section结束再按task数额外推进，组间有额外空距。section背景宽按任务数，content_right主要由task计算，空section未独立扩bounds。

忽略theme，固定12色section循环、无section每task换色，黑色箭头。period/event文字按byte len估算高度却实际只绘单text不换行；宽固定190，长文字可能溢出，不使用共享wrap。事件竖向堆叠，虚线长度按全图最大event估高，title也byte估宽。所有用户文本五种XML转义。图尺寸由估算bounds+padding决定，不是实际SVG文字bbox；没有任务数/输出大小cap。已记录，未构建，整体pending。

## journey完整读取

journey_diagram.rs完整读取。header精确journey，title/section前缀非空保存；task按所有冒号分片并剔除空片，前两项为name/i32 score，剩余以冒号空格重建后按逗号分actor。不验证score 1..5，不支持quoted冒号保护，空字段剔除可能改变位置；仅section无task由renderer拒绝。全局actor按首次出现去重，但task内重复actor仍绘多次。

flatten保留每task当前section，连续同名任务组成section，非连续同名可有多个section块；起始无section任务仍绘制而无section标题。固定横向200间距、task150x50、left150；参与者图例不扩left margin或图高。face y=300+(5-score)*30，>3笑/<3哭/=3平脸；越界score会落viewBox之外，尺寸仍以450底边计算。固定theme palette，传入theme未应用。

section/task标签提供foreignObject XHTML加switch text/tspan fallback，文本各自五种XML转义；不执行输入HTML。actor title、图例、title也转义。固定框与图尺寸不测量长文本，标题仅增加垂直空间；不能声称自动适应字体或全部actor数量。已记录源码证据，未构建或运行本包测试。

## quadrant完整读取

quadrant_diagram.rs完整读取。header精确quadrantChart，title覆盖，axis必须包含-->（两端可空，保留引号）；quadrant-N接受任意i32但renderer只取1..4，重复编号覆盖。point按首冒号和方括号内精确两项f64，label仅去配对双引号；不拒绝NaN/infinity，普通越界坐标render clamp0..1，NaN仍可能输出NaN。

必须至少一点，因此renderer的无points轴在顶部/象限文字居中分支从正常入口不可达。正常轴在下方、象限文字顶端，Q1右上/Q2左上/Q3左下/Q4右下；点标签在点下方，固定500x500和固定字体/边距，无重叠规避或文字测量。theme.background以#0或#1开头粗判dark，选整套固定palette，不按实际亮度且忽略其它theme颜色；背景字符串直接插入fill，用户文本则五种XML转义。未运行动态测试，整体pending。

## radar完整读取

radar_diagram.rs全部读取。header精确radar-beta，axis逗号拆分去空并覆盖前次；curve name {values}追加，忽略空数值项，f64不检查有限性。其它语句静默跳过，title未解析，固定输出空radarTitle。要求至少一个axis，不要求curve也不要求3轴；曲线维度不匹配时整条及图例跳过，但其值仍参与全图max计算。

max从全部series值fold f64::max后至少1，min固定0，负值max到0；infinity可使归一化出现NaN。固定700x700、半径300、5个圆网格、等角轴，闭合三次曲线tension0.17；1/2轴仍进入同公式，无数据/点数资源限额。

仅背景使用传入theme且直接插style，其它网格/轴常量；CSS只定义series0的curve/legend box，后续series虽输出对应class却无各自样式，不能宣称完整多色系列。axis/legend使用五种XML转义，文本不测量/换行/避让，图例不扩viewBox。当前只静态证据，未构建。

## sankey完整读取

sankey_diagram.rs完整读取。header精确sankey-beta，正文按裸逗号精确3列source/target/f64，非CSV引号解析；空节点名、负值、NaN、infinity未拒绝，至少一条link。节点按首次出现保存顺序，重复边保留。

节点值=max(入流和,出流和)，depth反复按pred最大+1更新，最多2*节点数轮；有环不会报错而使用最后一轮层数，依赖输入顺序，非拓扑合法性保证。按depth BTreeMap分层、层内原序，ky取各层可用高度/总流最小值，非有限退1；层内节点多到padding超过400时可得负ky，未clamp。节点高=value*ky、居中堆叠，边宽同样value*ky，原边顺序累计输入输出offset。

固定600x400、node宽10、padding25；节点颜色前10用palette，超出全部退首色而非循环。DOM id为first appearance index加1，最终节点按depth/y/name输出；标签name加节点值，近整数转i64。末层标签左侧，其余右侧；不测量/裁剪文字。边在节点与label之后绘，三次曲线中点控制、源目标渐变、multiply blend。主题只用background直接插style/fill，label无显式theme text color；文本/id/gradient色做XML转义。未构建，保持pending。

## state完整读取

state_diagram.rs完整读取。header接受精确stateDiagram/stateDiagram-v2，输出固定TopToBottom FlowchartGraph，不能配置direction。state声明只取首whitespace token作id，余文仅contains choice/fork/join marker决定Diamond/ForkJoin/默认RoundedRectangle；不是quoted alias或复合状态解析。声明可覆盖已存在shape但保留首次顺序。

迁移只split_once -->，右侧首冒号为label，不解析链或引号；[*]按source/target分别映射共享__start/__end，普通同名ID可碰撞，空端点未拒绝。未声明节点自动加入；Start/End/ForkJoin无label，其它直接id为label，不走flowchart normalize_label。全部node先输出，再按输入序输出Arrow edges。其余非空非%%行报Unrecognized，包括direction、description、复合状态结束括号等；state X {首行可被宽松接受但并未建子图。空header图允许交给通用layout。此处只转换AST，复合状态不应登记为支持；整体仍pending。

## mindmap完整读取

mindmap_diagram.rs完整读取。首个有效行必须mindmap，至少一个节点。缩进每个ASCII空格或tab计1，非tabstop；::装饰行整个跳过。节点用n0等内部ID，形状前用户ID丢弃；((圆))、{{六边形}}、))bang((、[矩形]、(圆角)按查找定形，Cloud枚举无解析入口。只有首节点默认圆，其余默认带底色圆角框。标签只trim，不解引号、实体或换行。

栈按缩进弹出并挂父节点；第二个同层根会重新挂到原根下，非多根报错。根每个直接子树分配section，后代继承，8色循环。UnicodeWidthStr乘8.5估宽，高16，无多行测量/换行；各形状固定padding。叶子数决定角度权重，第一层全圆，深层限扇角，半径随节点数/深度增大，无碰撞消除或深度限额。

只有根时布局提前返回中心(0,0)，SVG从0开始，负半边会被裁切；有子节点则整体按节点边界平移留20。边中心连中心，quadratic控制点正好中点实际为直线，宽随深度减小。主题参数未用，根蓝底白字，分支固定色；Cloud/Bang椭圆近似。非根非圆underline与填充同色，代码注释的互补色未实现。标签XML转义&<>双引号；节点内部id未输出SVG id。静态读取完成，尚无动态验证。

## gantt完整读取

gantt_diagram.rs完整读取。精确gantt header，title/section设置当前值，dateFormat行忽略；任务首冒号后逗号拆分去空，要求至少3项，按id/start/duration取前三项，多余忽略，不实现任务状态标记。after只引用先前完整ID，重复ID覆盖依赖表但保留两条任务；不支持前向、多依赖。时长i32加d/D或w/W，负数零接受；周乘7及日期/依赖加法无溢出保护。末字节split_at对多字节单位可panic。日期只校验三段整数，未校验有效月日，转换为civil day；至少一个任务。

固定宽784、高100+24*任务数，时间域最早start到最晚start+duration，每天一根tick，不抽稀或限额。负时长可产生负条宽，反向域cast usize会导致巨大循环；极端日期算术也无保护。section背景和标签按唯一分类汇总数量布局，但task行仍原始顺序，非连续重复分类时背景可能不匹配。task类取section序号mod4，而CSS仅定义task0；sectionTitle只定义0/1。文本按UTF8字节数乘字体估宽，按空间放条内/条左/条右，不换行或扩viewBox。theme未用，固定配色；任务、section、title五种XML转义。当前仅静态证据，整体仍pending。

## kanban完整读取

kanban_diagram.rs完整读取。精确kanban首token；未缩进行整行作column title（不解析id[label]），任何首字符Unicode whitespace即task，不比较层级。task先于column报错，header-only合法空board。任务首@{且行以}结束时按单行YAML mapping解析，否则整行保留label；无多行metadata输入。label/assigned/priority/ticket接受string/number/bool转字符串，其它类型和未知键忽略；非法YAML报原行号。ticket只存储不输出链接或文本。

列宽200间距5，卡185宽、44高，有assigned即56高（空string亦是）；整体高度随最高任务栈增长，无文字换行和限额。assigned右对齐按UTF8字节数乘9.5估宽，长字符串不扩卡。所有标签已改原生SVG text，旧foreignObject注释不代表当前实现。column title/task label直接作为转义后DOM id，重复可碰撞，不生成独立身份。

priority精确Very High红/High橙/Low蓝/Very Low浅蓝，Medium及未知不画标线。列section从1起，但CSS仅生成0..10，11列起无专用填色；section_hsl虽有mod12不等于渲染循环覆盖。背景和卡固定白；theme只text_color/node_stroke直接插CSS，不采用背景/填色等其它字段。文本、id五种XML转义；标题和任务不支持HTML。完整静态读取，尚无本包测试，pending保留。

## block完整读取

block_diagram.rs全部547行读取。header精确block-beta；columns前缀匹配（不要求空格），auto和任意负数单行，正数按行分列，非法值忽略保留旧值；0被接受并在有节点布局时除零panic。block/end分组、style/classDef/class/linkStyle、space占位全跳过，未实现嵌套、样式、空位。每行一个节点或首-->分割一条边，不支持链/同一行多节点/边标签。节点首[拆id/label，结尾]可缺，去配对单双引号；无[整行作id。standalone解析失败静默跳过，edge端失败报错。首次ID确定顺序，后续label!=id才覆盖，不能显式恢复与ID相同的标签。

UnicodeWidthStr乘10.97加8估宽，固定文本高19加8；全部节点归一到最大尺寸，按声明顺序排矩形网格。不换行。空图被接受但无节点bounds保留正负Infinity，生成非法viewBox。边用矩形射线交点和末端退4、三点basis path，全部实线单向箭头；额外圆/叉marker虽输出但无解析入口。自环不单独路由，bounds不计边或标记。theme背景/文本/边/填色/边框直接插CSS/style，subgraph字段未用。

节点id和文本、边id的from/to有XML转义；边class里的ls/le却直接来自小写原ID，未转义。由于裸ID任意字符串，该路径不能宣称SVG属性全转义。SVG固定my-svg ID亦可能在同页面重复。数字path保留至3位去尾零。仅静态证据，风险登记待独立处理，不在本次改代码。

## gitgraph完整读取

gitgraph_diagram.rs全部576行读取。header精确gitGraph，其余header内容忽略（方向无效）；默认main空head。commit生成内部序号ID并以当前head为唯一父，忽略所有commit参数（id/tag/type等）。branch只取下一个whitespace token，首次创建继承当前head但不自动checkout；重复创建无操作。checkout未知分支自动创建空head分支，不继承旧head。merge总新建Merge节点，收集当前及目标head；未知目标不报错，自合并可重复父，无fast-forward判定。其它命令报错；header-only合法。名称不解引号，命令尾部token忽略。

分支按创建序固定y间隔90，每commit按全局序x=10+50*seq；各分支从0画到全局末尾，分支创建时间不截线。父边同层直线，跨层20半径折弧；merge边颜色用父分支，其它用目标分支。普通commit单圆且画旋转-45标签，merge双圆无标签。标签是序号与确定性wrapping hash组合，并非用户commit id/真实Git hash。分支宽度按UnicodeWidthStr乘9.5近似，标签bounds近似不测量真实旋转边界，无避让或资源限额。

CSS仅0..7颜色集，SVG使用未mod的分支序号，因此第9分支起没有专用配色，branch_color fallback并不补发规则。主题background/text/edge/node_fill直接插CSS，其余节点字段未用；标签及分支文本五种XML转义。tag/reverse/highlight样式存在但语法无对应行为。完整静态审阅，未启动本包测试。

## xychart完整读取

xychart_diagram.rs全部867行含11项测试读取。header精确xychart-beta，title及axis按前缀，只有line（关键字边界为空/whitespace/[）数值系列；bar和其它未知行忽略。至少一个非空line，混合空系列保留序号但不渲染。数值首[与末]之间逗号分割，空项跳过，前后杂文忽略；未验证end>start，反序括号可slice panic。f64允许非有限值；没有验证系列长度一致。

x轴分类列表按单双引号保护逗号但不处理反斜杠escape/未闭引号错误，去空类别；以首[末]确定列表，尾文忽略。可选标题去一层配对引号。数值轴首-->分左右，左末whitespace token作min，其前为标题；数值可反向/相等/非有限。无x轴默认数值0..0，标题-only x为空类别。y标题-only不清除前次显式range；无显式range由全部系列min/max，常值扩±1，非有限极值退0..0。数据本身不清洗，NaN/Inf仍可输出path。

固定700x500，留标题及轴文字估算带宽；y轴按Unicode宽度乘font*.525，x分类以max(类别数,最长系列长,1)分band；类别只画已有标签，数据较长仍在图内。类别字体按最宽标签缩至8..14，达到8后仍可能溢出，不旋转/换行。数值x所有系列按最长系列数共用索引等距位置（不是输入x值），单点在左端；默认0..0 tick只左端但多数据点仍跨宽度。y线性缩放不clamp，超显式范围可越界且无clipPath；相等域全部在底端。

D3近似tick以1/2/5/10步进，目标10，反向range逆序，非有限range无tick；数字标签最多6小数可能舍入极小值为0。折线M/L无marker，单点仅M不可见；Tableau10按原系列索引循环，无图例或bar绘制。theme背景和文本颜色直接插style/fill/stroke，其它颜色无效。用户文本五种XML转义。11测试覆盖分类解析和双线渲染、numeric smoke、dark轴色、y自动范围、缺图/空line/错误header、分类越界长度、numeric单点和共用索引；不覆盖非法括号、非有限值、bar语义或图像像素。此处是读取测试代码，尚未执行本包测试。

## requirement完整读取

requirement_diagram.rs全部874行读取。精确requirementDiagram header，direction默认TB，大小写不敏感接受TB/TD/BT/LR/RL，非法值报错。节点kind大小写不敏感：requirement、functional/interface/performance/physicalRequirement、designConstraint及element。name只首whitespace token，不解析带空格引号名；同名声明覆盖。开{可同头行或下一有效行；同头行后续属性丢弃，闭}必须后续行起始；EOF无闭括号也接受，甚至无开括号到EOF仍可创建空属性节点。

属性逐行首冒号，尾逗号去除再去单双引号；重复key后写覆盖，未知key忽略。requirement支持id/text/risk及verifyMethod优先verifymethod；element支持type及docref优先docRef。risk低中高、验证analysis/demonstration/inspection/test忽略大小写规范首字母；其它值照存，无枚举拒绝。空属性不显示，正文固定ID/Text/Risk/Verification或Type/Doc Ref顺序。两行标题+20gap+24每正文行，宽用UnicodeWidthStr*8+20，无换行；存在正文才分隔线。

关系首->或<-拆分，去独立- token后取src/type和另一端，未知relation type接受；多余token部分忽略，未校验端点声明。graphlib会创建隐式节点但无node_metrics，因此不绘这些节点。Dagre directed multigraph noncompound，nodesep/ranksep50、edge20、margin8，f64尺寸转f32；节点由HashMap迭代插入，不能宣称稳定顺序布局。关系不设name，同向同端点覆盖底层边，输出仍每条关系一份path/label并复用末次geometry/尺寸，标签文字却取原关系。边折线M/L，节点/边/标签bounds整体平移留8，空图16x16。

contains精确小写时实线且无末箭头；虽然定义containsStart marker，实际没有marker-start引用，不能登记圆加号包含标记已渲染。其它关系统一10,7虚线+开放末箭头。关系标签<<type>>带固定浅灰背景；节点最终按ID排序输出不代表布局顺序已排序。theme五个通用颜色直接插style/attribute，用户id/文本五种XML转义。无本文件测试，只有静态审阅，未宣称动态覆盖。

## ER完整读取

er_diagram.rs全部936行读取。header erDiagram精确首token，空输入内部parse也可返回空图（公共dispatch另有边界）；固定TB布局，无direction语法。entity头行必须以{结尾，整个前缀作名称，不解引号/alias；属性到独立}或EOF，缺闭括号不报错。属性首whitespace分type与余下完整name，单token作type空name，PK/FK/comment等未建独立字段。同名实体后块覆盖前块。游离}忽略，裸实体声明报错；关系引用自动补无属性实体。

关系以首冒号取role（不去引号），左侧whitespace至少3项取实体A/关系token/实体B，多余忽略。--识别关系实线，..非识别关系8,8虚线。cardinality接受||、|o/o|、|{/}|、o{/}o，两端对应四种基数，不额外限制符号朝向；无效token报原行。以{结尾的文本先命中entity块，解析并非完整grammar。标签不执行HTML或实体解码。

节点按BTreeMap ID排序，UnicodeWidthStr*8估宽，最小100；标题和属性行高42.75，type/name两列按各自最大宽加padding布局，无wrap。Dagre directed multigraph noncompound、TB、nodesep140/ranksep80/edge20/margin8，f32几何；每关系无独立edge name，同端点覆盖底层边后每个原关系仍重复使用末次geometry与label尺寸。输出两端基数marker，card_a在start、card_b在end；关系label用主题文字色，无背景。节点/edgepoints/labelbounds归一留8，marker外沿不单独计入，空图16x16。

实体行交替色：dark判定background前三字节RGB加权亮度<128，则向白混8%/16%；light为白与node_fill混25%。纵向type/name分隔线在行填色后绘制。颜色辅助仅按字符串byte长度至少6后slice0..2/2..4/4..6，不支持短hex/CSS函数；非ASCII可能切UTF8边界panic，非法hex分别按255判亮或0混色，尾部额外内容忽略。marker圆仍硬编码white；CSS marker fill none!important另可能覆盖。主题颜色直接插SVG/CSS，用户文本/id五种XML转义。该文件无测试；lib已有ER dark和基数测试此前读过，尚未执行本包动态验证。

## class完整读取

class_diagram.rs全部1144行读取。classDiagram精确header，direction大小写不敏感TB/TD/BT/LR/RL，其它InvalidDirection。class整行剩余字符串作名称，未解析alias/generic语义；尾{块读到独立}或EOF，缺闭括号接受。成员含(即method，否则attribute，保留可见性等原文本不解释。块内完整<<...>>设置最后stereotype；重复块追加成员但stereotype被本块值覆盖（无则清除）。Name: member单行也创建类并追加，但stereotype此路径只是attribute。引用自动补空类；其它裸声明/namespace等报错。

关系在空白token内匹配18种运算符，不接受无空格连接。仅操作符相邻独立双引号token识别端点cardinality，不支持其中空格；card_from、关系label、card_to合并成中央一个字符串，不在各端定位。左右额外token报错，冒号首处分割非quote-aware。反向符号--|>、--*、--o、<--等只归为同RelationType，未交换from/to或marker端；因此无法宣称遵循符号指向。点线composition/aggregation也归为实线类型，点线样式丢失。Extension/Realization三角始端、Composition/Aggregation菱形始端、Dependency/DashedDep箭头末端；Association无marker。lollipop及多余反端marker定义不代表语法支持。

BTreeMap按ID插节点，Dagre四方向，nodesep/ranksep50、edge20、margin8，尺寸f64->f32。无独立edge name，同端点多关系底层覆盖并复用geometry；label文本/width取各原关系。类框UnicodeWidthStr*8估宽，title按18/14放大，padding12，成员行27，空区保留6与额外gap；两条分隔线总绘制。stereotype显示«...»。无换行或真实字体测量。

viewBox宽高只取节点bounds跨度+16并至少100，未额外normalize，未纳入edge/label/markerbounds；输出整数viewBox。边在节点之后绘制，重新按矩形求交点，始marker沿首末向量退17、末箭头退6（不是沿局部段），无短边长度上限；折线一位小数。标签nodefill背景75%opacity，中央label绘制；无edge points整边跳过。theme空nodefill/stroke/edge/text回固定默认，其它原值直接插CSS/属性；background完全未用。全局svg CSS和固定marker ID未隔离多图。用户文本/id五种XML转义；该文件无单测，lib class smoke/cardinality已有静态读取，未运行本包测试。

## C4完整读取

c4_diagram.rs全部1201行读取。精确五种C4Context/Container/Component/Dynamic/Deployment header共享解析与布局，只有Dynamic关系标签加序号。Person/Person_Ext；System/Container/Component及Db/Queue和_Ext共20类按参数位置创建，缺参补空，不校验alias唯一性，重复节点全部保留。Person/System第三参description，Container/Component第三techn第四description，多余参数忽略。title保留引号。

函数行首(找func，再按双引号toggle及括号depth找匹配)，只处理单行，尾部忽略。args对双引号外且括号depth0逗号分割，去所有双引号，trim；中间空项保留而尾空项丢弃，不处理escape或单引号。未知/畸形调用和其它未知行静默跳过，三个Update*Style/LayoutConfig也忽略。

Boundary/Enterprise/System/Container_Boundary及Deployment_Node/_L/_R记录parent并入栈，无需真正尾{；独立}/end出栈，多余关闭忽略，EOF未闭也接受。布局忽略boundary.parent_boundary：global节点先排，所有有直属shape的boundary按声明序竖直追加；没有直属shape（即只含子boundary）的外框不画，非真正嵌套。alias global与内建global冲突可改变归属，重复alias会反复排同组。shape最小216x60，UTF8字节数*.6font估宽，description/technology各单行增高，无wrap；type文字宽不纳入尺寸。每行最多4且绝对width_limit800触发换行，宽节点仍可超限，没有自动压缩。

普通节点圆角矩形，Db竖圆柱、Queue横圆柱；Person嵌入静态48x48 PNG data URI，不请求网络。外部person常量含非ASCII字符，不能仅凭常量存在保证PNG可解码。节点按类型固定蓝/灰，文字固定白，边界固定灰框黑字；theme仅SVG背景、基础文字及marker CSS部分使用。类型显示内部snake_case，图标symbol computer/database/clock虽定义但没有use引用。所有shape通用person-man class，alias不输出DOM id。

Rel方向变体/Rel_Back/Rel_Neighbor全部归rel，BiRel变体归birel但renderer不使用rel_type，实际统一单向末箭头，方向提示无布局效应。只find首同alias shape，未知端点关系跳过，不自动补节点；关系到boundary亦不支持。按原关系索引第一条直线、后续quadratic，即使第一条被跳过仍不重新选直线；Dynamic编号同原索引可留空缺。交点公式以top-left比较而非中心向量，不能声称精确所有尺寸交点；曲线无避障。标签固定几何中点、techn下一行，不测量入bounds。

viewBox由shape/screenbounds确定，标题额外高60但x=(内容宽/2)-200可为负，未按文字扩边；Db/Queue外凸和关系label也未纳入精确bounds。文本XML转义&<>双引号（文本中单引号无需转义），主题字符串直接插CSS。固定my-svg及marker/icon ID多图可冲突。该文件无测试，当前静态读取完成，本包仍pending。

## sequence完整读取

sequence_diagram.rs全部1326行读取。header精确sequenceDiagram；正文关键字大多ASCII大小写不敏感，但边界只空格/tab。participant/actor相同矩形呈现，create只注册不控制出生时间，destroy只resolve不截生命线；box只单bool跳过分组头尾，无背景/嵌套盒语义。accTitle/accDescr前缀整个忽略，跨行{块直到含}；links/link/properties空格前缀忽略。无participants补Participant。空输入内部parser也可补默认（公共入口另有header边界）。

声明仅精确小写空格as空格分割；去两端任意单双引号，不校验配对。ID选择启发式：有空白者为label，无空白者为ID，否则UTF8字节更短者ID、同长选左。两侧均作为alias。注册找首ID/label/alias相交项合并，保留原ID和顺序，只在旧label==id时升级label；其余label成为alias。同名展示label可能合并不同ID。消息、note、activation引用均可自动创建参与者，空引用亦未拒绝。

消息首冒号分头/正文，按固定优先序查找10种箭头：双向<<-->>/<<->>、实/虚filled ->>/-->>、cross -x/--x、open -)/--)、无头 ->/-->；非quote-aware，不按最早箭头位置，剩余头文本作为端点。target前+激活目标、前-停用源，只认一个。消息后追加activation事件；正文不调用decode，title和note才把#59;转分号。autonumber默认关、next1/step1；off停用，空参恢复现值，u64可设置start/step（0步长允许），无效数字保留旧，saturating_add到上限重复。仅消息编号，空正文变n.。

note over单人/首逗号两人、left/right of单人，必须冒号，不解多逗号/quoted冒号。alt/loop/opt/par/critical/break/rect压栈，else/and/option全部同分隔事件，无类型约束；游离else仍增28高度但不画分隔，游离end无操作，未闭片段自动闭。消息解析先于fragment，标签若含箭头可能被当消息。rect颜色文本保存但renderer无tab且不显示label，不应用背景色。

事件行37、fragment header28/footer12；activate/deactivate不占行，取上次消息/note中心，跨fragment时仍可指向旧行。activation每参与者栈后进先出，未配对deactivate忽略，未闭延伸内容底端，最小高6、宽8、嵌套x偏4。fragment覆盖所有参与者跨度，与实际涉及对象无关，每深度向内18，不clamp宽度，深嵌套可负宽；片段紫色虚线和tab文本固定色，其余文本theme。排序先外层后内层，activation绘在片段之前。

所有参与者统一最大label框宽，UnicodeWidthStr*7.2+16最小100；只精确<br/>分行，任意多行header仍统一44（单行32），无按行数继续增高。相邻默认box宽+30，跨参与者消息估宽*0.95均摊增距，宽最少360，高最少220。title按估宽*1.25扩总宽但不重排参与者。自消息右向cubic宽26/高15.54，仅末marker，双向始marker丢失；其文字宽不计入总宽，仅留50。普通消息单行居中无wrap。

note固定高26黄底黑字；over宽按两生命线跨度+100最小120，与文字无关；beside估宽最小120最大380，左侧clamp8可盖生命线且不增左margin，右侧扩总宽。片段标签和激活嵌套不计width，可能溢出。生命线顶部到底部实细线，全部参与者头尾框均绘制。theme五色直接插属性，文本五种XML转义，固定seq marker ID多图不隔离。该文件无单测，lib已有扩展语法测试此前静态读取，本包尚未动态执行。

## 通用SVG renderer完整读取

svg_renderer.rs全部990行读取。render默认配置，render_with_config采用fontFamily、有效fontSize、flowchart wrappingWidth和curve；仅不区分大小写linear直线，其余全basis。是否state不是源diagram类型而是任意StartState/EndState/ForkJoin节点存在，故仅普通状态节点时使用flowchart字符宽8，有特殊节点才全图6.7。输出XML声明、整数width/height/viewBox和背景rect/style，未重算布局bounds。顺序子图背景→边→按ID排序节点→子图标题→全部边标签。

12shape均有输出分支：矩形、rx5圆角、height/2 stadium、菱形、min(width,height)/2圆、六边形height/3切角、圆柱ellipse半径min(width/8,height/4)、subroutine内侧8线、asymmetric五边形、start实圆、end双圆、fork矩形。asymmetric实际顶点向左突出，注释称V-notch不能替代几何事实，文字偏右height/8。普通shape应用节点fill/stroke覆盖再theme；start/fork只theme edge，end外nodeStroke内background且不画label，不支持任意节点文字色/边宽。节点id不输出SVG属性。

文字共享wrap算法，按font*1.1行距中心排tspan，fontSize输出取整数但布局仍小数；fontFamily与文字五种XML转义，颜色直接插属性。子图背景用theme subgraph色，title按默认字符宽而非state宽换行，顶部无额外margin。背景raw string末尾\n是字面反斜杠n而非换行。未消费securityLevel/htmlLabels等配置，不代表HTML执行路径存在。

边少于2点不画；六种style决定箭头/点线/粗线，普通1、粗3.5，点线3 3，圆端圆join。箭头固定marker8或11，末段长度大于4/5.5才退相应距离，短末段不退（无法保证所有箭头尖端恰贴边）。basis先处理轴对齐且两邻段>5的直角，增加距角5的两点和修正角点，再三次basis；linear直接M/L。坐标一位小数，固定marker ID无多图隔离。

边标签空白跳过；显式label_pos仅x>0且y>0才使用，否则按fix_corners后折线累计长度中点（linear模式也fix_corners，非最终曲线弧长）。有label_pos但无有效点可回到0,0。共享wrap后labelbounds加水平/垂直各4；两两矩形额外分离8，最多10轮沿最小重叠轴各移一半，顺序影响结果，不保证收敛。仅label间避让，未检查节点、子图标题、边、viewBox，移动后不更新布局尺寸。固定浅灰rgba(.8)背景不随theme改变，文本theme。该文件无测试，本轮完整静态读取，尚未动态验证。

## 通用layout读取进度：1–1200/3826

layout.rs前1200行完整读取，其余尚未读，以下仅登记已见实现。入口默认和带config均center_subgraph_nodes=true，另有内部no-centering入口。options取node/rankSpacing、padding、wrappingWidth和有效fontSize；state检测递归查特殊Start/End/ForkJoin形状，非diagram显式类型。主流程collect→cluster分析/backedge检测→Dagre→子图内部rank修正→state snap/terminal对齐→可选子图居中及解重叠→节点和子图框→边路由/裁剪→shift和最终bounds。被调后续函数尚待读取，不提前确认其算法。

可见主流程对自环单独构造、裁剪并取折线中点label；非自环先取Dagre点或obstacle fallback，按几何backedge判定使用专用route，否则straighten。cluster端点非backedge先去内部点再裁剪。非空label优先Dagre位置仅当落在路径axis-aligned bounds±8，否则折线中点；backedge始终中点。最终左上shift仅依据子图、节点、labelbounds，未遍历edgepoints最小值；右下则计全部edgepoints和labelbounds，因此不能宣称负向边路径全在viewBox内。renderer后续label碰撞位移另不在此bounds。

子图Mermaid顺序使用root开始有visited的postorder再reverse；bottom-up另一DFS没有visited。ancestor遍历同样无visited，若重复ID形成父关系环存在不终止风险，需结合collector核对。居中连通组只看直接所属子图之间的边，不看cluster ID端点；以全部成员跨轴平均，再对每子图直辖节点整体平移至组均值。先用直接节点+padding/title估框，任一非祖孙框交叠则整组跳过。居中移动仅positions未同步Dagre边点。标题估高wrap且至少24，不据标题宽扩框。

解子图重叠按非祖孙两两估计subtree框，沿跨轴把中心较大者正移overlap+25，最多子图数平方轮；未收敛不报错，不是注释所说保证直到无交叠。shift_subgraph移动所有子树nodes，并仅当原edge两端都是子树node时同步Dagre点和label；cluster端点及跨界边不随动。最终子图框bottom-up纳入直辖nodes和已padding子框，各级加padding8和标题高度，纯空子图省略；输出保持原subgraphs顺序。

ClusterAnalysis计算每子图直辖节点及有无外部edge，但build参数_cluster_analysis未用。Dagre为directed/multigraph/compound，edgeSep20 margin8，state longest-path，其余默认ranker。子图先0尺寸padding8，普通nodes按order，parent按order设置，set_parent错误忽略。预先检测backedge跳过；cluster endpoint另转成员，转后自环跳过。label尺寸由后续helper待查，edge全部name None，即便multigraph也复用同端点一条底层边。extract及其余2626行待下轮继续。

## 通用layout续读：1201–2100/3826

extract按每个原edge索引读取同端点无name Dagre边，因此重复边复用布局已确认。state snap取全部positions的y排序去重(<.5)，用去backedge后的最长路径rank索引映射，level不足整个跳过；固定操作y不按graph方向切轴。terminal singleton要求rank>0、该层唯一、无forward outgoing且至少两个前层入边，将x设前驱最大x（重复边可满足数量），不是前驱平均。拓扑rank按节点order取ready，未处理节点保留0，后继max+1。

collector顺序扫描，is_subgraph_id只看已收集子图：后声明cluster可能先被edge补成普通节点，且加入子图时未移除旧nodes，存在顺序相关冲突。节点第一次在子图出现时设置owner，之后不迁移；同节点先在root再在子图可被认领。子图ID无唯一校验，可嵌套同ID；结合前述无visited bottom-up DFS确认循环递归风险。subgraph title缺省ID。style后statement整体替换properties，fill/stroke取该列表第一匹配key，其它样式不生效。

add_node若已存在且新label None直接返回（shape也不更新）；有label可覆盖shape/label/尺寸但保留order。ensure只补rectangle。cluster Dagre端点只考虑直辖node：source取最后无内部outgoing，target取首无内部incoming，环则退最后/首；纯子cluster父级无直辖返回None，调用侧退原cluster ID。渲染端点先普通node后cluster虚拟rectangle，因此普通node/cluster同ID优先普通node。

node尺寸：state圆角max(textw+6,32)×max(texth+16,40)，diamond取w/h+18及40最大正方，start/end14、fork70×7；普通rect textw+4padding、texth+2padding，圆角各2padding，subroutine w+padding+16/h+padding，asym额外h/4，hex宽(textw+2.5padding)*7/6，diamond两轴(w+padding)+(h+padding)，circle只textwidth+padding决定直径（忽略多行高度），stadium多h/4，cylinder按宽计算ry加3ry高度。state特殊shape总存在使普通分支End20/Fork70×10通过当前engine不可达。文本和labelwrap共享font缩放，label两轴加4。

存在dead_code compute传统布局入口，公共入口未调用；其assign_ranks迭代cap=nodes*max(edges,1)，忽略DFS backedges、未到达归0。detect_back_edges虽然allow(dead_code)实际被主Dagre流程调用：无入边roots来自HashMap未排序，若无root退首edge.from，否则无边退词序首node；其余未访问节点词序扫描，递归DFS按邻边输入序记录栈内回边，未设深度限制。不能根据allow(dead_code)把helper判为不可达。group_by_rank按order，compute_positions方向分派仅传统入口，vertical前半按带label的跨rank边给每gap至少24额外间距，余下位置逻辑待读取。

## 通用layout续读：2101–3826，整文件完成

传统非入口路径的vertical逐rank居中，rankSep固定50+label gap24；horizontal跨rank补__dummy_N__（无名称冲突防护），4轮barycenter排序，然后按rank Laplacian求offset，固定rank0、无pivot交换、近零pivot跳过，不保证奇异系统求解。normalize按中心最小值及全图最大半尺寸留8。传统separate仅top-level排序，垂直y交叠比例>.5则向右，否则沿主轴移，固定padding20/title25；shift_external只看y，把外部节点移出所有top-level合并纵向带。另两个未从入口调用的居中/chain对齐helper会删跨组edgepoints或内部edgepoints以触发重算；不登记为当前公共pipeline能力。

主路径compute_bounds无positions时200x200，否则只普通nodes右下+8；只有cluster positions可8x8。edge_label_midpoint累计直线段长度半点。几何backedge按方向逆向差>10，区别于建图DFS回边集。fallback障碍路由在所有非端点node的扩10矩形中取首个与端点轴对齐bounds交叠者（HashMap顺序影响选择），仅绕这个障碍向近侧外30，没有重验所有障碍或子图框；无障碍时三点折线以目标横/纵坐标及主轴中点构造并相邻dedup。不能宣称全局无碰撞。

主backedge垂直依据from.x>=to.x选右侧否则左侧、端点外30，未检查中间nodes；水平聚合端点x跨度±30内所有节点上/下bounds，选距两端均值较近侧。simple fallback则总右/下60。smooth-U九点固定start主轴减、end加，不为BT/RL反向改公式。自环5点，size clamp(width*.35,40,60)，垂直向右、水平向上，后者负y若无label可能未被左上shift补偿。

straighten仅跨轴差<15且至少2点时尝试均值轴直线，用其他node扩5矩形交叉检测拒绝；只判首末线段，不是任意polyline/曲线碰撞检测。segment-rect有same-side快判、端点内判和四边交点，平行跳过。fix_subgraph_internal_ranks只在任一直辖内部非自环edge主轴差<1时启动，拓扑maxrank重排，环残留rank0；maxrank0跳过。间隔固定50而非options rankSpacing，按原节点平均中心平移，沿主轴正向递增，不为BT/RL反转，且不改Dagre edgepoints。

cluster trim采用严格内部判定（边界算外），保留外点旁一个内部过渡、最少两点；全部在内部没有outside时保持原状，先target后source。clip改首末点，circle/state用圆射线、diamond菱形方程，其余包括hex/cylinder/asymmetric/stadium都按矩形近似。零向量默认on_node顶部、towards右侧；零尺寸diamond分母可能NaN，未统一有限性检查。

6项本文件测试仅覆盖cluster trim：目标尾部、最少两点、全外不变、源头部、双cluster及边界点保留。不是布局整体正确性测试；目前只读未运行。至此layout.rs 3826行全部审阅，主路径与传统未调用路径区分完毕，本包其余移植文件待核查。

## mermaid_port完整读取

flow_parser6、flow_db131、flow_data75、cluster_adjust373、dagre_layout_port569行完整读取，mod40此前已读。parser直接转调默认parser，无独立语法支持。FlowDb子图后序登记、FlowData倒序groups再vertex_order；style会创建节点并认领owner，properties跨statement追加，后续ported fold最后fill/stroke获胜，区别默认layout整体替换且列表首项获胜。节点显式label覆盖shape，无label不覆盖；owner首次，子图ID仅查已登记列表，同样顺序相关。groups与vertices可同ID，node_meta后写覆盖。

cluster adjust找所有有children的节点，递归descendants及anchor叶（优先无common edge，否则最后reserve），descendants计算不计自身。外部连接以edge两端是否descendant异或；无外连保留cluster端点，有外连改anchor，改写时保留edge name并标anchor parent外连。common edge替换公式对入边w==id1仍设id1而非id2，按实际代码记录。extractor只提无外连cluster，depth>10停止，但前置descendants/findchild/copy递归无限深保护。copy搬节点/parent/相关edge再从父graph删节点，edge_in_cluster使用任一端descendant而非两端。内图rankdir父tb→lr，其他→tb，之后只覆盖node/rankSpacing选项。多层抽取状态沿用初始analysis。

ported layout directed multigraph compound，node_meta按id、edge_meta按端点，所有set_edge name None，同端点最终只保留一份最后关系。adjust改写端点后extract仍按原edge_meta键找，键不匹配就跳过边，未同步meta。先递归子图Dagre得到尺寸填父cluster，再父Dagre，将子布局按父左上平移并合并；HashMap遍历子图，未保证子图输出稳定顺序。尺寸直接取graph.width/height，未作默认layout的重叠修复/裁剪/回边路由/label最终bounds补偿。group无正尺寸省略，不预留title高度；节点尺寸同普通flowchart分支，无state专用度量。默认lib的enabled常量false，使此路径当前不从公开渲染入口启用，不能登记为用户可选引擎。

ported3测试覆盖rankSpacing增大、padding/wrap改变尺寸、font32扩大节点和边label；未测cluster抽取或关系保持。至此本包35份Rust源码全部静态审阅完成，尚待契约映射、证据登记及动态测试；保留pending，不能仅凭阅读完自动算完成。


## 正式契约登记

- [Mermaid render dispatch](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-render-dispatch)
- [Mermaid theme precedence](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-theme-precedence)
- [Mermaid language fence recognition](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-language-fence-recognition)
- [Mermaid frontmatter boundary](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-frontmatter-boundary)
- [Mermaid invalid YAML stripping](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-invalid-yaml-stripping)
- [Mermaid configuration coercion](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-configuration-coercion)
- [Mermaid font size normalization](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-font-size-normalization)
- [Mermaid stored configuration limits](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-stored-configuration-limits)
- [Mermaid state configuration boundary](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-state-configuration-boundary)
- [Mermaid disabled experimental port](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-disabled-experimental-port)
- [Mermaid Unicode width estimation](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-unicode-width-estimation)
- [Mermaid word wrapping](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-word-wrapping)
- [Mermaid long token split](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-long-token-split)
- [Mermaid wrapped height](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-wrapped-height)
- [Mermaid flowchart declaration](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-flowchart-declaration)
- [Mermaid flowchart statement termination](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-flowchart-statement-termination)
- [Mermaid flowchart edge chains](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-flowchart-edge-chains)
- [Mermaid flowchart label normalization](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-flowchart-label-normalization)
- [Mermaid flowchart node shapes](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-flowchart-node-shapes)
- [Mermaid flowchart subgraph identifiers](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-flowchart-subgraph-identifiers)
- [Mermaid flowchart style parsing](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-flowchart-style-parsing)
- [Mermaid node declaration replacement](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-node-declaration-replacement)
- [Mermaid subgraph ownership](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-subgraph-ownership)
- [Mermaid effective node styles](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-effective-node-styles)
- [Mermaid Dagre graph and duplicate edges](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-dagre-graph-and-duplicate-edges)
- [Mermaid cycle edge detection](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-cycle-edge-detection)
- [Mermaid cluster endpoint anchors](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-cluster-endpoint-anchors)
- [Mermaid nested subgraph bounds](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-nested-subgraph-bounds)
- [Mermaid connected subgraph centering](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-connected-subgraph-centering)
- [Mermaid subgraph overlap resolution](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-subgraph-overlap-resolution)
- [Mermaid state rank adjustment](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-state-rank-adjustment)
- [Mermaid state terminal alignment](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-state-terminal-alignment)
- [Mermaid shape measurement](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-shape-measurement)
- [Mermaid collapsed internal ranks](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-collapsed-internal-ranks)
- [Mermaid obstacle routing limits](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-obstacle-routing-limits)
- [Mermaid back edge routes](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-back-edge-routes)
- [Mermaid self loop routes](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-self-loop-routes)
- [Mermaid aligned edge straightening](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-aligned-edge-straightening)
- [Mermaid cluster interior route trimming](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-cluster-interior-route-trimming)
- [Mermaid shape clipping approximation](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-shape-clipping-approximation)
- [Mermaid layout final bounds](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-layout-final-bounds)
- [Mermaid info fixed output](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-info-fixed-output)
- [Mermaid pie input and slices](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-pie-input-and-slices)
- [Mermaid pie legend and theme](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-pie-legend-and-theme)
- [Mermaid packet bit rows](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-packet-bit-rows)
- [Mermaid timeline section grouping](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-timeline-section-grouping)
- [Mermaid timeline sizing](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-timeline-sizing)
- [Mermaid journey scores and actors](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-journey-scores-and-actors)
- [Mermaid journey label fallback](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-journey-label-fallback)
- [Mermaid quadrant coordinates](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-quadrant-coordinates)
- [Mermaid radar axis and curves](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-radar-axis-and-curves)
- [Mermaid sankey input and cycles](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-sankey-input-and-cycles)
- [Mermaid sankey scale and palette](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-sankey-scale-and-palette)
- [Mermaid mindmap indentation](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-mindmap-indentation)
- [Mermaid mindmap radial layout](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-mindmap-radial-layout)
- [Mermaid gantt dependencies](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-gantt-dependencies)
- [Mermaid gantt calendar limits](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-gantt-calendar-limits)
- [Mermaid gantt render scale](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-gantt-render-scale)
- [Mermaid kanban grammar](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-kanban-grammar)
- [Mermaid kanban metadata](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-kanban-metadata)
- [Mermaid kanban card rendering](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-kanban-card-rendering)
- [Mermaid block grammar](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-block-grammar)
- [Mermaid block node labels](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-block-node-labels)
- [Mermaid block grid geometry](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-block-grid-geometry)
- [Mermaid block attribute escaping](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-block-attribute-escaping)
- [Mermaid gitgraph branch operations](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-gitgraph-branch-operations)
- [Mermaid gitgraph commit identity](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-gitgraph-commit-identity)
- [Mermaid gitgraph merge semantics](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-gitgraph-merge-semantics)
- [Mermaid gitgraph visual layout](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-gitgraph-visual-layout)
- [Mermaid XY supported plots](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-xy-supported-plots)
- [Mermaid XY category parsing](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-xy-category-parsing)
- [Mermaid XY numeric domains](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-xy-numeric-domains)
- [Mermaid XY series alignment](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-xy-series-alignment)
- [Mermaid XY plot presentation](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-xy-plot-presentation)
- [Mermaid XY malformed number lists](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-xy-malformed-number-lists)
- [Mermaid state declarations](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-state-declarations)
- [Mermaid state transitions](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-state-transitions)
- [Mermaid requirement node kinds](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-requirement-node-kinds)
- [Mermaid requirement properties](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-requirement-properties)
- [Mermaid requirement relationships](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-requirement-relationships)
- [Mermaid requirement layout](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-requirement-layout)
- [Mermaid requirement contains marker](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-requirement-contains-marker)
- [Mermaid ER entity attributes](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-er-entity-attributes)
- [Mermaid ER cardinality](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-er-cardinality)
- [Mermaid ER layout and labels](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-er-layout-and-labels)
- [Mermaid ER dark stripes](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-er-dark-stripes)
- [Mermaid class declarations](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-class-declarations)
- [Mermaid class relation operators](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-class-relation-operators)
- [Mermaid class cardinality labels](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-class-cardinality-labels)
- [Mermaid class box sizing](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-class-box-sizing)
- [Mermaid class layout bounds](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-class-layout-bounds)
- [Mermaid class edge clipping](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-class-edge-clipping)
- [Mermaid class theme boundary](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-class-theme-boundary)
- [Mermaid C4 declarations](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-c4-declarations)
- [Mermaid C4 function arguments](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-c4-function-arguments)
- [Mermaid C4 boundaries](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-c4-boundaries)
- [Mermaid C4 relations](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-c4-relations)
- [Mermaid C4 dynamic numbering](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-c4-dynamic-numbering)
- [Mermaid C4 presentation](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-c4-presentation)
- [Mermaid sequence participants](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-sequence-participants)
- [Mermaid sequence alias merging](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-sequence-alias-merging)
- [Mermaid sequence lifecycle limits](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-sequence-lifecycle-limits)
- [Mermaid sequence message arrows](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-sequence-message-arrows)
- [Mermaid sequence autonumber](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-sequence-autonumber)
- [Mermaid sequence note grammar](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-sequence-note-grammar)
- [Mermaid sequence fragments](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-sequence-fragments)
- [Mermaid sequence activations](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-sequence-activations)
- [Mermaid sequence fragment geometry](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-sequence-fragment-geometry)
- [Mermaid sequence sizing](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-sequence-sizing)
- [Mermaid SVG paint order](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-svg-paint-order)
- [Mermaid SVG shape rendering](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-svg-shape-rendering)
- [Mermaid SVG text options](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-svg-text-options)
- [Mermaid SVG edge styles](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-svg-edge-styles)
- [Mermaid SVG curve mode](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-svg-curve-mode)
- [Mermaid SVG label placement](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-svg-label-placement)
- [Mermaid SVG label background](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-svg-label-background)
- [Mermaid port database boundary](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-port-database-boundary)
- [Mermaid port flow data order](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-port-flow-data-order)
- [Mermaid port cluster extraction](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-port-cluster-extraction)
- [Mermaid port recursive layout](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-port-recursive-layout)
- [Mermaid port options and dimensions](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-port-options-and-dimensions)
- [Mermaid theme preset parsing](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-theme-preset-parsing)
- [Mermaid theme variable aliases](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-theme-variable-aliases)
- [Mermaid theme sparse overrides](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-theme-sparse-overrides)
- [Mermaid public error surface](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-public-error-surface)
- [Mermaid vendored library boundary](../specs/mermaid-svg-rendering/spec.md#requirement-mermaid-vendored-library-boundary)
