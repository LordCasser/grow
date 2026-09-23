## ADDED Requirements

### Requirement: Default pure Rust diagram engine
default_engine SHALL 返回共享的 Send + Sync PureRustEngine，调用 vendored mermaid-to-svg 生成 SVG，再使用公共 rasterize 输出 PNG 与实际像素尺寸；不自动探测或切换 mmdc。

#### Scenario: 支持范围证据
- **WHEN** 渲染本包测试中的 flowchart、sequence、class 与 xychart-beta 示例
- **THEN** 产生可解码 PNG；sequence 覆盖激活、自调用、note 与 Unicode，class 覆盖泛型与关系，xychart 覆盖分类轴与多折线。完整语法范围继续由 vendored 包核查，不能推导所有 Mermaid 语法均支持。

#### Scenario: 循环与长标识
- **WHEN** 渲染循环登录流程或长标识示例
- **THEN** SVG 保留节点标签和箭头，长标识保持单个 tspan；同进程重复渲染相同输入和参数具有确定性。

证据：`crates/codegen/mermaid/src/lib.rs` — `default_engine`；`crates/codegen/mermaid/src/pure.rs` — `build_svg`；`crates/codegen/mermaid/tests/pure_engine.rs` — `xychart`。

### Requirement: Checked source limit and unwind boundary
render_checked SHALL 先按 UTF-8 字节长度检查 RenderLimits.max_source_bytes（默认 64 KiB），再通过 catch_unwind 调用 engine；超过上限返回 Parse，普通成功和错误原样传递。

#### Scenario: 边界长度
- **WHEN** source.len 恰等于上限
- **THEN** 允许进入 engine；超限在调用 engine 前拒绝。

#### Scenario: 引擎 panic
- **WHEN** 引擎产生可展开 panic
- **THEN** 返回 Panic，提取字符串 payload，否则使用默认消息；warn 只记消息长度，debug 可含消息。本封装不捕获 abort/OOM/栈溢出，也不实施运行时间上限，直接 engine.render 不经过此大小检查。

证据：`crates/codegen/mermaid/src/engine.rs` — `render_checked`；`crates/codegen/mermaid/src/engine.rs` — `panic_message`；`crates/codegen/mermaid/src/engine.rs` — `RenderLimits`。

### Requirement: Diagram error classification
纯 Rust 适配层 SHALL 将 vendor ParseError、InvalidDirection、InvalidNodeShape 映射为 Parse，DotGeneration/RenderError 映射为 Layout，UnsupportedDiagramType 映射为 Unsupported，并保留错误文本。

#### Scenario: SVG 输出失败
- **WHEN** SVG 解析、尺寸、分配或 PNG 编码失败
- **THEN** 由 raster 层返回 Rasterize；枚举另有 Timeout/Panic 供进程与 checked 层使用。

证据：`crates/codegen/mermaid/src/pure.rs` — `map_engine_error`；`crates/codegen/mermaid/src/engine.rs` — `MermaidError`；`crates/codegen/mermaid/src/raster.rs` — `rasterize_with_font`。

### Requirement: Diagram themes and viewer parameters
RenderParams SHALL 默认 Light、目标宽度 1024、最大高度 4096、scale 1、最小宽度 0、background None；Light/Dark surface 分别为 #FAFAFA/#18181B，纯 Rust SVG 主题使用对应 surface。

#### Scenario: 系统查看器
- **WHEN** 调用 for_os_viewer
- **THEN** 目标宽度为 0，scale 为 2，采用调用方最小宽度/最大高度并填入主题背景。

#### Scenario: 透明与颜色
- **WHEN** 单独 rasterize 且 background 为 None 或 Some
- **THEN** None 不预填背景；Some 保留 RGBA 的 alpha。SVG 自己仍可绘制不透明背景，None 不保证最终 PNG 透明；Rgba.to_hex 仅输出 RGB。

证据：`crates/codegen/mermaid/src/lib.rs` — `RenderParams`；`crates/codegen/mermaid/src/lib.rs` — `for_os_viewer`；`crates/codegen/mermaid/src/pure.rs` — `theme_for`；`crates/codegen/mermaid/src/raster.rs` — `rgba_to_color`。

### Requirement: Raster size and allocation bounds
栅格化 SHALL 先按非零目标宽度缩放，否则使用 scale；非有限或非正 scale 回退 1，再提高到最小宽度、按非零最大高度和 32 百万像素面积缩小。

#### Scenario: 极端尺寸
- **WHEN** 取整或极端长宽比可能越过面积约束
- **THEN** 每轴限定 1..16384，必要时再次缩小较长轴，确保最终 width*height 不超过 32000000。最小宽度服从这些上限。

#### Scenario: 最终几何
- **WHEN** 最终整数尺寸确定
- **THEN** 分别缩放两个轴以填满 pixmap，极端比例或上限处理可改变宽高比，不承诺严格保形；返回的尺寸与 PNG 一致。

证据：`crates/codegen/mermaid/src/raster.rs` — `effective_scale`；`crates/codegen/mermaid/src/raster.rs` — `clamp_dimensions`；`crates/codegen/mermaid/src/raster.rs` — `MAX_OUTPUT_MEGAPIXELS`。

### Requirement: Bundled font shaping and Unicode fallback
栅格化 SHALL 嵌入 Roboto-Regular 并将 generic font family 指向该字体，固定首选 shaping face；ASCII SVG 只用缓存的 bundled 数据库，含非 ASCII 时使用另一个加载系统字体的缓存数据库以补足 glyph。

#### Scenario: 指定系统字体名
- **WHEN** SVG 请求 Arial 等系统 family
- **THEN** 首选仍为 bundled face，避免已有 Latin glyph 被系统字体替换。

#### Scenario: 中文字符
- **WHEN** bundled 字体缺字
- **THEN** 由系统字体 fallback 决定能否显示；主机无覆盖字体时不保证有中文字形，也不承诺不同主机输出字节一致。字体文件及其许可告知保留在 assets。

证据：`crates/codegen/mermaid/src/raster.rs` — `build_font_set`；`crates/codegen/mermaid/src/raster.rs` — `font_set_for`；`crates/codegen/mermaid/src/raster.rs` — `pinned_resolver`。

### Requirement: SVG image resource resolution
rasterize SHALL 将 usvg 的字符串 image href resolver 替换为始终 None，拒绝通过该入口读取外部文件或网络图片，保留内存 data URL 解析。

#### Scenario: 资源边界
- **WHEN** SVG 指向外部 image href
- **THEN** 不通过该 resolver 读取目标；此限制不等同于通用沙箱，Unicode 系统字体仍可加载，也不约束外部 mmdc/Chromium 的执行。

证据：`crates/codegen/mermaid/src/raster.rs` — `resolve_string`。

### Requirement: Explicit mmdc rendering
MmdcEngine SHALL 由 caller 显式传入 binary 或通过 PATH detect 创建，默认进程等待预算 1500ms，可通过 with_timeout 修改；在临时目录写源文本，调用 mmdc 输出 SVG 后使用公共 rasterize。

#### Scenario: 调用参数与隐私
- **WHEN** 执行外部引擎
- **THEN** 使用 input/output/outputFormat svg/theme 参数，Light 映射 default、Dark 映射 dark；stdin/stdout/stderr 为 null，应用 pager_env 与 detach。Unix 源文件以 create_new 和 0600 创建。

#### Scenario: 失败分类
- **WHEN** 命令缺失、超时、非零退出或无 SVG 输出
- **THEN** 分别返回 Unsupported、Timeout、Layout、Layout；等待错误和临时文件写入失败为 Rasterize。检测不验证浏览器安装，测试 fake mmdc 不证明真实 Chromium 可用。

证据：`crates/codegen/mermaid/src/mmdc.rs` — `MmdcEngine`；`crates/codegen/mermaid/src/mmdc.rs` — `detect_mmdc`；`crates/codegen/mermaid/src/mmdc.rs` — `map_subprocess_error`；`crates/codegen/mermaid/src/mmdc.rs` — `write_private`。

### Requirement: Render subprocess input and spawn retry
run_with_timeout SHALL 使用 caller 已配置的 Command，ExecutableFileBusy 时最多尝试启动 5 次，中间依次等待 20/40/60/80ms；成功 spawn 后用 scoped thread 写可选 stdin payload，并等待子进程。

#### Scenario: 大 payload
- **WHEN** caller 配置 piped stdin 并提供 payload
- **THEN** writer 与等待并行，忽略写入错误并关闭 stdin；其他 spawn 错误直接返回 Spawn。

#### Scenario: 错误调用
- **WHEN** 提供 payload 但 stdin 未 piped
- **THEN** 记录警告，debug 构建在 spawn 后触发断言，release 丢弃 payload；此误用路径没有通用回收 guard，caller 必须遵守前置条件。

证据：`crates/codegen/mermaid/src/subprocess.rs` — `run_with_timeout`；`crates/codegen/mermaid/src/subprocess.rs` — `spawn_with_etxtbsy_retry`。

### Requirement: Render subprocess exit cleanup
run_with_timeout SHALL 对正常/非零退出均尝试清理 Unix detached 进程组，对 timeout/wait 错误尝试 group kill、direct child kill 并 wait；仅零退出返回 Ok，其他情况保持错误分类。

#### Scenario: 平台与隔离前提
- **WHEN** caller 已用 detach_std_command 建立子进程组
- **THEN** Unix 按 child PID 调用 killpg(SIGKILL)，仅覆盖该组且忽略清理错误；非 Unix 没有 group kill，超时/错误只终止直接子进程，不能保证 Chromium 后代全部结束。

#### Scenario: 预算边界
- **WHEN** 等待预算到期但存在组外后代或启动退避
- **THEN** 预算覆盖 wait_timeout，不包括 spawn 退避，scoped writer 仍须 join；持有 stdin 的逃逸后代可延迟返回，因此不承诺任意 Command 的整个调用严格在预算内结束。

证据：`crates/codegen/mermaid/src/subprocess.rs` — `wait_and_reap`；`crates/codegen/mermaid/src/subprocess.rs` — `reap`；`crates/codegen/mermaid/src/subprocess.rs` — `reap_process_group`。

### Requirement: Terminal Mermaid dispatch
markdown 内置 Mermaid SHALL 在 mermaid fence 中尝试 flowchart/graph、state、class、ER 和 sequence 五类终端文字渲染；空源返回 None，不支持或布局超限使用原文框回退。

#### Scenario: 触发条件
- **WHEN** code info 首 token 大小写无关为 mermaid 且正文 end 小于输入长度
- **THEN** 尝试该文字渲染路径；此条件独立于最终 closed code metadata 检测。

#### Scenario: 图宽度
- **WHEN** 指定 max_table_width
- **THEN** 同一预算传给 Mermaid 布局；本路径不生成图片。

证据：`crates/codegen/markdown/src/mermaid.rs` — `parse_graph`；`crates/codegen/markdown/src/mermaid.rs` — `parse_sequence`；`crates/codegen/markdown/src/parse.rs` — `try_push_mermaid`。

### Requirement: Terminal Mermaid flow syntax
flow parser SHALL 支持方向、带形状 label 的节点、链式或成组端点、不同线型和箭头，以及带标签边；解析为受限文字图而非完整 Mermaid 语言。

#### Scenario: 数量上限
- **WHEN** 节点超过 128 或边超过 512
- **THEN** 解析路径回退；源 statement 字符串在计数检查之前收集，不能推导总输入内存上限。

#### Scenario: 选择形状
- **WHEN** 节点 shape 为 Diamond
- **THEN** 元数据保留该形状，但当前 draw_box 与 Round 一样画圆角框。

证据：`crates/codegen/markdown/src/mermaid.rs` — `parse_statement`；`crates/codegen/markdown/src/mermaid.rs` — `parse_link`；`crates/codegen/markdown/src/mermaid.rs` — `draw_box`。

### Requirement: Terminal Mermaid label cleaning
图 label SHALL 清理有限 HTML 格式标签、引号和 Markdown 标记，再单次解码有限 named/numeric entity；普通 label 按显示宽度折行和省略。

#### Scenario: 长标识符
- **WHEN** label 超过 24 列
- **THEN** 优先在 _ - . / 后断开，否则逐字符，最多显示 4 行；构建过程可先产生更多行。

#### Scenario: 实体包含分号
- **WHEN** 无引号 statement 中有实体分号
- **THEN** statement splitter 可先拆开，不能由直接 label helper 测试声称完整 parser 接受该形式。

证据：`crates/codegen/markdown/src/mermaid.rs` — `clean_label`；`crates/codegen/markdown/src/mermaid.rs` — `wrap_label`；`crates/codegen/markdown/src/mermaid.rs` — `split_statements`。

### Requirement: Terminal Mermaid subgraphs
subgraph SHALL 记录嵌套作用域和首次归属，限制 24 个组及深度 6；将 group proxy 和跨组边提升至共同祖先作用域并绘制组框。

#### Scenario: 空子图
- **WHEN** 组内无有效节点且不被引用
- **THEN** 可以移除空组。

#### Scenario: 组内方向和宽度
- **WHEN** subgraph 声明自己的方向或外层宽度预算
- **THEN** 组内方向被忽略；子画布布局不传外层宽度，仅外层最终比较预算。

证据：`crates/codegen/markdown/src/mermaid.rs` — `parse_subgraph_decl`；`crates/codegen/markdown/src/mermaid.rs` — `render_grouped`；`crates/codegen/markdown/src/mermaid.rs` — `build_scope`。

### Requirement: Terminal Mermaid state diagrams
state parser SHALL 支持 state 声明、描述、转换标签及初始/终止伪节点，将 composite state 扁平为图；部分 metadata 和 note 被忽略。

#### Scenario: choice state
- **WHEN** 声明 choice
- **THEN** 记录 Diamond 元数据，但最终终端绘制仍受通用 draw_box 限制。

#### Scenario: 复合状态
- **WHEN** state 块中含子状态
- **THEN** 不建立与 flow subgraph 同等的嵌套可视化语义。

证据：`crates/codegen/markdown/src/mermaid.rs` — `parse_state`；`crates/codegen/markdown/src/mermaid.rs` — `parse_state_decl`。

### Requirement: Terminal Mermaid class and ER diagrams
class/ER SHALL 通过分区节点显示名称、成员或属性及关系；class 支持 annotation、generic 显示及有限关系符，ER 将 cardinality 转为文字标签。

#### Scenario: 成员上限
- **WHEN** 字段或方法超过各 8 条
- **THEN** 显示有限成员并附省略号。

#### Scenario: ER 空格 alias
- **WHEN** 关系实体 token 中包含带空格的 alias
- **THEN** 当前按空白切 token 的关系 parser 不能保证接受；现有 contains 文本测试可能仅看到 fallback。

证据：`crates/codegen/markdown/src/mermaid.rs` — `parse_class`；`crates/codegen/markdown/src/mermaid.rs` — `parse_er`；`crates/codegen/markdown/src/mermaid.rs` — `render_class`。

### Requirement: Terminal Mermaid graph layout
终端图布局 SHALL 以去灰色回边的 DFS 构建分层，保留原循环边绘制侧通道；通过最多 8 轮重心排序与 10 轮位置松弛布局，支持 TD/LR 及最终 BT/RL 翻转。

#### Scenario: 交叉或回边
- **WHEN** 边跨非相邻层或返回先前层
- **THEN** 使用侧轨道；不承诺全局最优或同构规范布局。

#### Scenario: 边标签冲突
- **WHEN** place_label 遇到已有字符或占用
- **THEN** 停止写入而不另寻位置；多条 self-loop 可共用路径，不能保证所有 label 和端点同时完整显示。

证据：`crates/codegen/markdown/src/mermaid.rs` — `compute_ranks`；`crates/codegen/markdown/src/mermaid.rs` — `order_ranks`；`crates/codegen/markdown/src/mermaid.rs` — `place_label`。

### Requirement: Terminal Mermaid canvas bounds
正常图和 sequence 布局 SHALL 在最终 Canvas 分配前检查宽度及单画布 2^21 格限制；宽字符用 continuation 哨兵表示并在输出时跳过。

#### Scenario: 单画布超限
- **WHEN** 计算尺寸超过 2^21 格
- **THEN** 返回 oversize 并回退，不为该最终画布分配。

#### Scenario: 全流程资源
- **WHEN** 解析或嵌套子画布先产生数据
- **THEN** 单画布限制不约束所有字符串、子画布同时存活或 fallback 的总体内存；字符绘制不等同 grapheme-aware 布局。

证据：`crates/codegen/markdown/src/mermaid.rs` — `MAX_CANVAS_CELLS`；`crates/codegen/markdown/src/mermaid.rs` — `layout_canvas`；`crates/codegen/markdown/src/mermaid.rs` — `layout_sequence`。

### Requirement: Terminal Mermaid sequence events
sequence SHALL 支持 participant/actor、隐式参与者、八类消息操作符、自消息、单行 note、autonumber 和有限控制块分隔线；限制 128 个参与者和 512 个事件。

#### Scenario: autonumber 参数
- **WHEN** 命令携带起始值或步长
- **THEN** 当前只开启 bool，自 1 计数，忽略参数。

#### Scenario: 生命周期与控制块
- **WHEN** 遇到 activate/deactivate 或 rect/box
- **THEN** activation 不执行；rect/box 只影响内部块栈而不画区域，栈没有独立深度上限。

证据：`crates/codegen/markdown/src/mermaid.rs` — `parse_sequence`；`crates/codegen/markdown/src/mermaid.rs` — `SEQ_OPS`。

### Requirement: Terminal Mermaid sequence layout
sequence SHALL 按参与者间距需求布置消息与 note，顶部和底部重复参与者框，并绘制 lifeline；note over 最多使用前两个参与者。

#### Scenario: 多行注释形式
- **WHEN** 输入超出单行 note 语法
- **THEN** 不支持一般多行 note 块；可用 note 采用固定三行框。

#### Scenario: 宽消息
- **WHEN** 消息或注释文本很长
- **THEN** 完整文本宽度影响间距与最终宽度检查；参与者显示 label 另按 24 列限制。

证据：`crates/codegen/markdown/src/mermaid.rs` — `parse_note_anchor`；`crates/codegen/markdown/src/mermaid.rs` — `layout_sequence`。

### Requirement: Terminal Mermaid fallback output
fallback SHALL 把原源行放入文字框，仅宽度失败时附打开图片的文字提示；unsupported 或 cell 超限不附该提示。

#### Scenario: 极窄预算
- **WHEN** max_width 小于标题或最小 body 宽度
- **THEN** body limit 至少 8 且标题不裁剪，最终框可超过预算。

#### Scenario: 大量原文
- **WHEN** fallback 源行很多
- **THEN** 当前没有总行数或格数 cap；提示不创建图片或可点击打开行为。

证据：`crates/codegen/markdown/src/mermaid.rs` — `fallback`；`crates/codegen/markdown/src/mermaid.rs` — `TOO_WIDE_HINT`。
