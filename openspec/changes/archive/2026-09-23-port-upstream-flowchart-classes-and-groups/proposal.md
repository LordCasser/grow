## Why

Grow 的纯 Rust Mermaid 引擎把模型生成的 flowchart 渲染为终端图片。当前 vendored parser 会把 `A & B --> C` 当作一个节点，并在 `PIN{label}:::class` 等常见写法下丢失标签或形状；上游 grok-build 已修复这些问题，但 Grow 的布局与安全边界已经独立演进，不能整包替换。

## What Changes

- 移植并适配 `classDef` 与节点尾部 `:::class` 的 flowchart 子集，保留形状、标签及 `fill`、`stroke`、`color` 的静态 SVG 表达；显式 `style` 优先于 class 样式。
- 将带空格的 `&` 节点组展开为相邻组间的边，保留引号和形状内的字面 `&`；对展开边数设硬预算，超限明确失败并交由现有失败/源码回退路径处理，而不是生成部分图片。
- 修正形状定界符的识别顺序，并让静态预览不支持的 flowchart 指令不再伪装成节点；输出到 SVG 属性的颜色值必须转义。
- 增加 vendored parser/SVG 测试与 Grow PNG/worker 回归，保留 Grow 自有的子图布局、CJK 和进程隔离修补。

## Capabilities

### Modified Capabilities

- `client-surfaces`: Mermaid flowchart 预览应正确呈现上述常见节点与边语法，并在异常展开时安全回退。

## Impact

- 主要实现：`third_party/mermaid-to-svg/src/{parser,layout,svg_renderer,lib}.rs`，必要时更新同包 vendoring 说明；调用链是 `crates/codegen/mermaid/src/pure.rs` → `crates/codegen/pager/src/app/agent_view/mermaid_worker.rs`。
- 测试：vendored 包及 `crates/codegen/mermaid/tests/pure_engine.rs`；现有 pager 的 64 KiB 输入、3 秒子进程期限与 PNG 像素上限继续有效，但不能替代解析期的边数预算。
- 非目标：同步上游整个 vendored crate 或依赖栈、实现完整 Mermaid class/交互语法、重写 Grow 的布局算法、顺带处理其他图表类型。
- 上游参考：`xai-org/grok-build@4247f66` 的 `third_party/mermaid-to-svg` 局部修补；具体行为以本 change 的 delta 为准。
