## 1. Baseline and regression cases

- [x] 1.1 复核当前 vendored parser、Grow 布局补丁、PureRustEngine 和 Pager worker 的调用/失败路径；以源码位置与预修复失败用例记录证据，确认不覆盖并行改动。
- [x] 1.2 为 `:::class` 标签/形状、显式 style、字面 `&`、不支持指令和分组边建立 parser/SVG 回归测试；记录预修复源码证据与实际测试结果。

## 2. Scoped vendor port

- [x] 2.1 移植 `classDef`、内联 class 与首形状定界符识别，保持 Grow 既有 parser 扩展；运行 vendored class/标签/形状测试确认语义。
- [x] 2.2 实现形状/引号外的 `&` 节点组和边链笛卡尔展开，并在生成前用 checked 算术累计 4096 条分组边预算；用 4096/4097、跨多行累计与字面 `&` 测试验证，整数溢出由 checked 运算拒绝。
- [x] 2.3 让 `color` 仅进入节点文字、保留显式 style 优先，并对节点 SVG 颜色属性执行 XML 转义；运行形状、主题、SVG 结构及恶意属性值测试。
- [x] 2.4 更新 vendored `Cargo.toml` 的本地修补记录，核对 Grow 既有布局/CJK/open-label 与禁用 port 未丢失；以 diff 审阅和 `cargo test --locked -p mermaid-to-svg --lib` 验证。

## 3. Integration and closure

- [x] 3.1 增加并运行 `cargo test --locked -p mermaid` 的 PNG 回归，以像素确认三种颜色在 Light/Dark 主题可见；标签和边由 parser/SVG 回归确认。
- [x] 3.2 运行 Pager Mermaid worker 的超额输入与失败路径测试，确认无部分产物且保留原有回退/错误提示；运行受影响的布局、CJK 和子图测试。
- [x] 3.3 更新相关开发者说明（链接本 delta，不复制另一份需求），将测试命令、退出码、平台和未验证边界写入本 change 的 `verification.md`；执行 `git diff --check` 与 `openspec validate --all --strict --no-interactive`。
- [x] 3.4 核对每个 WHEN/THEN 已由实现及验证覆盖，确认可归档；未完成场景不得勾选或归档。
