## ADDED Requirements

### Requirement: Flowchart class annotations preserve node meaning

Grow 的 Mermaid flowchart 静态预览 SHALL 在识别节点尾部 `:::class` 时保留节点原有 ID、形状和标签，并将已定义 `classDef` 的 `fill`、`stroke`、`color` 分别用于节点填充、边框和文字。节点的显式 `style` SHALL 优先于 class 样式；无法静态执行的交互类指令 SHALL 不变成伪节点。本要求只覆盖 `classDef` 与内联 `:::` 子集，不承诺完整 Mermaid class 语法。

#### Scenario: 带形状和 API 路径的 class 节点
- **WHEN** flowchart 包含 `PIN{machine claimed?}:::cond` 和 `CALL["POST /v1/items/{id}"]:::dark`，并定义对应 `classDef`
- **THEN** 预览保留菱形、矩形及完整标签，已定义的填充、边框和文字颜色体现在 SVG/PNG 中，不以 ID 或被截断的标签替代。

#### Scenario: 显式 style 与 class 并存
- **WHEN** 同一节点同时引用 `classDef` 并拥有显式 `style` 语句
- **THEN** 显式 `style` 继续决定该节点的有效样式，class 不在解析结束时覆盖它。

#### Scenario: 不支持的静态指令
- **WHEN** flowchart 含 `class`、`click`、`linkStyle`、`direction`、`accTitle` 或 `accDescr` 指令
- **THEN** 静态预览不将这些整行指令画成节点；本要求不意味着指令的交互、链接或方向语义已经实现。

#### Scenario: 样式值包含 SVG 属性定界字符
- **WHEN** `classDef` 或既有 `style` 的颜色值含引号、尖括号或 `&`
- **THEN** 输出 SVG 的颜色属性保持 XML 结构完整，不因该值新增属性或元素；有效普通颜色仍按原值显示。

目标实现入口：`third_party/mermaid-to-svg/src/parser.rs` — `parse_statements`、`try_parse_node`；`src/layout.rs` — `get_node_colors`；`src/svg_renderer.rs` — `render_text_lines` 与各节点形状渲染器。

### Requirement: Flowchart ampersand groups represent individual edges

Grow 的 Mermaid flowchart 静态预览 SHALL 把形状与引号之外、两侧有空白的 `&` 解释为节点组分隔符，将每对相邻组展开为笛卡尔积边；形状或引号内的 `&` SHALL 保留为标签内容，不得生成包含整组文本的伪节点。

#### Scenario: 分组连接和边链
- **WHEN** 输入为 `A & B --> C --> D & E`
- **THEN** 图中有 `A→C`、`B→C`、`C→D`、`C→E` 四条逻辑边，节点 ID 不包含 `A & B` 或 `D & E`。

#### Scenario: 标签中的字面 ampersand
- **WHEN** 输入含 `REP["GET /v1/items?sku=&status=READY"]:::dark`
- **THEN** 标签中的 `&` 保留，不拆成多个节点或边。

目标实现入口：`third_party/mermaid-to-svg/src/parser.rs` — `parse_edge_chain`、节点解析；`crates/codegen/mermaid/src/pure.rs` — `PureRustEngine::render`。

### Requirement: Flowchart group expansion is bounded before allocation

单个 flowchart 中由 `&` 节点组产生的逻辑边 SHALL 至多为 4096 条。解析器 SHALL 在构造超额边之前拒绝输入，并返回可由现有 Mermaid 错误路径处理的解析失败；失败 SHALL 不发布部分 SVG 或 PNG。

#### Scenario: 刚好达到预算
- **WHEN** 分组语句累计恰好展开 4096 条逻辑边
- **THEN** 解析阶段接受该展开，不因边数预算单独拒绝；后续布局和像素限制仍独立适用。

#### Scenario: 超过预算或乘积溢出
- **WHEN** 新组连接会使累计展开边数超过 4096，或组大小乘积无法安全计算
- **THEN** 在构造该组边之前返回解析失败；Pager 走现有渲染失败路径，不展示不完整图片。

目标实现入口：`third_party/mermaid-to-svg/src/parser.rs` — `Parser::parse_edge_chain`；`crates/codegen/pager/src/app/agent_view/mermaid_worker.rs` — 渲染失败处理。
