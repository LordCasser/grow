## Context

见 proposal.md。Grow 的 `PureRustEngine` 调用 vendored `render_mermaid_to_svg`；Pager 把不可信 Mermaid 源码送到短命子进程，输入默认限 64 KiB、等待 3 秒，栅格化另限 32 MP。当前 `parser.rs` 与上游修复前的版本基本同源，但 Grow 的 `layout.rs` 已有独立子图与回边修补，所以直接复制上游目录会覆盖本地行为。归档的 Mermaid 逐包审计是历史证据，不是主规范；当前主规范尚无这组 flowchart 行为要求。

## Goals / Non-Goals

**Goals:** 在现有 parser → layout → SVG → PNG 路径内修复 class 与分组语义；让分组的超线性展开在分配前有确定上限；保持现有 worker 失败分类和预览边界。

**Non-Goals:** 不新增渲染引擎或配置项，不启用实验性 dagre port，不实现独立 `class A dark` 指令、点击/链接行为或完整 CSS，也不处理其他图表语法债务。

## Decisions

1. **按文件/函数移植，不替换 vendored 基线。** 对照本地上游 `third_party/mermaid-to-svg` 的 parser、layout、SVG 变化，只移植本 delta 所需逻辑。保留 Grow 的布局补丁和 `graphlib_rust` 依赖形态；`LayoutNode` 新增文字颜色时，所有构造点（含恒禁用的 port）只做字段适配。`Cargo.toml` 的 vendoring notes 追加来源与本地差异，不覆盖原有 1–12 项说明。另一种方案是整包复制，但会把不同 Warp revision 和 Grow 布局算法一并回退。
2. **解析一次，末尾应用 class 样式。** Parser 保存 `classDef` 与节点内联 class 引用，解析结束后生成现有 `Statement::Style`；已出现显式 `style` 的节点不再附加 class style，保留当前“后一个 style 整体替换”的布局语义。修正 `extract_node_id` 为按源码顺序寻找形状开启定界符、跳过双引号；`:::` 与 `&` 都只在形状/引号外解释。这样不需要新增 AST 样式体系。独立 `class` 等当前无法执行的指令按静态预览跳过，不能假装已经实现。
3. **在边链展开点计预算。** `Parser` 持有本图由分组产生的边数；任一侧是多节点组时，在双重循环前用 `checked_mul` 和 `checked_add` 计算将新增的边。预算为 4096，超过则返回 `MermaidError::ParseError`，整图不返回 AST。普通单节点边不计入这项新预算，以免顺带改变已有大型图的接纳范围。Pager 的 64 KiB、3 秒与 Linux 地址空间上限仍是独立后备，不被误称为解析期内存上界。
4. **颜色只经原有静态属性通道。** `fill`/`stroke` 继续从有效 `style` 进入布局，`color` 只影响节点文字，默认主题文字色保持原值。SVG 输出边界对节点填充、边框和文字颜色做 XML 属性转义，包括既有显式 `style`；不把原始值作为标签或额外属性拼入 SVG。无须为了三项颜色建立 CSS 解释器。
5. **失败沿用既有返回链。** vendored parser 失败由 `PureRustEngine` 映射为 Parse；worker 不写出成功产物，Pager 按原有失败/源码回退或 Open Image 错误提示处理。不能把进程超时当作预算测试的替代；预算测试直接调用 parser 并断言没有部分 AST。

## Risks / Trade-offs

- [上游 parser 与 Grow 局部补丁交错] → 按语义移植并回归 CJK、子图、循环边、open edge label 和其他图表；不以 `git apply` 成功作为验收。
- [class 样式优先级并非完整 Mermaid CSS 层叠] → 明确只支持内联 class 子集和显式 style 优先；不静默宣称独立 `class` 语法可用。
- [4096 条边仍可能触发布局超时] → 保留现有 worker 隔离；预算只承诺限制分组解析阶段的超线性边分配，不承诺任意图 3 秒内绘完。
- [XML 转义只保证属性结构，不验证所有 CSS paint 值] → 保留 usvg 的既有图片资源拒绝边界；不把本 change 说成全 SVG 安全审计。

## Migration Plan

无需数据迁移。分阶段先加失败复现，再移植 parser、布局字段和 SVG 输出，最后验证 PNG 与 Pager 失败路径。若回归显示布局或平台渲染倒退，撤回本次局部移植并保留原有 vendored 版本；不修改主规范直到验证、归档完成。
