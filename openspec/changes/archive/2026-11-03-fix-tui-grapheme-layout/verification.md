## Reproduction

环境：macOS，Rust/Cargo 1.93.1（与 rust-toolchain.toml 一致）；用户确认 iTerm2、Fullscreen、FiraCode Nerd Font Mono、关闭 ambiguous double width。默认 iTerm2 profile 的只读核对与此一致：`FiraCodeNFM-Reg 14`，`Use Non-ASCII Font=false`、`Ambiguous Double Width=false`。未修改终端设置。

先只增加三个 `grapheme_layout` 回归并运行旧生产实现：3 failed / 0 passed。

- `中⚠️文` 在宽度 3 下产出 `中⚠️`，实际为 4 列。
- `a⚠️b` 截断到 3 列仍返回完整的 4 列原文。
- `中⚠️👨‍💻é尾` 中 `尾` 的起始列错误地算成 8，实际为 7。

## Code verification

全部 Cargo 验证使用 `PATH=/Users/lordcasser/.cargo/bin:$PATH CARGO_INCREMENTAL=0`；测试另设 `RUST_MIN_STACK=16777216`，不新增 incremental 编译缓存。

- `cargo test --locked -q -p pager-render --lib grapheme_layout -- --nocapture`：5 passed。覆盖中英/圈号混排、VS16、肤色修饰与 ZWJ、组合音标、宽度 1–12、joiner 重建、搜索 byte ranges、显示列、带样式 Buffer 绘制和截断。普通文本另以 textwrap 为 oracle，在宽度 0–16 下对 FirstFit/OptimalFit 校验空串、纯空白、首尾空格、连字符、中文标点和硬换行。
- `cargo test --locked -q -p pager-render -p ratatui-inline --lib -- --test-threads=4`：pager-render 1027 passed / 1 ignored；ratatui-inline 52 passed。包含既有 OSC8、宽字 lead cell、原生提交及 resize 用例。
- `rustfmt --edition 2024 --check crates/codegen/pager-render/src/render/wrapping.rs crates/codegen/pager-render/src/render/line_utils.rs`：通过。
- `git diff --check`：通过。
- `cargo check --locked -q -p pager-render`：通过，核对默认生产依赖图。
- `openspec validate --all --strict --no-interactive`：归档前 17 passed / 0 failed。
- 归档后全量严格校验：16 passed / 0 failed；`openspec validate --archived --no-interactive`：798 passed / 0 failed。首次归档校验仅因归档任务未勾选失败，完成勾选后重跑通过。
- 清理本次产生的三组测试可执行文件及调试对象共 123.9 MiB；保留共享依赖缓存和原有 incremental 缓存，未清理其他任务的构建产物。

## Screenshot and font boundary

用一次性 CoreText Swift 探针测量系统字体，临时脚本已删除：FiraCodeNFM-Reg 14 的 ASCII 单列 advance 为约 8.615pt；`①/②` 均回退 HiraginoSans-W3，advance 为 14pt，ink x=0.294..13.706pt，超出该单列右缘约 5.091pt。中文 `中` 回退 PingFangSC-Regular。这支持圈号 fallback 字形伸入相邻 cell 的推断，但不等同于已确认 iTerm2 raster/clip/cursor 的最终像素行为。

额外测量的本机 Menlo/Monaco/MapleMono 字体也将圈号回退到同一 Hiragino 字体，没有获得可直接推荐的已验证替代字体。未执行真实 iTerm2 前后对比、PTY 全应用回归或 release 重建；因此不宣称用户截图中的圈号重叠已完全修复。iTerm2 可单独指定 non-ASCII 字体，见 [官方字体说明](https://iterm2.com/documentation-fonts.html)；候选字体还需确认包含单宽圈号字形。

## Adversarial review

只读独立复核指出 textwrap Custom callback 仍接收原生 Word 的 scalar-width API，以及跨 span 的组合字素不能由局部 helper 定义正确绘制归属。生产调用均使用内置 FirstFit；本次将契约边界写明，没有新建 callback API 或强行合并样式。ANSI 原生历史分行、工具 cursor model 和跨 span 的后续范围登记在 backlog。搜索命中组合符号之后的列已覆盖；只匹配字素内部 scalar 的高亮语义未扩展。
