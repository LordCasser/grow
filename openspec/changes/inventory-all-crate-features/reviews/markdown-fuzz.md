# markdown-fuzz 逐包核查

包路径：`crates/codegen/markdown/fuzz`。全部所属 Rust 模块和 Cargo.toml 已阅读；测试执行边界见本文件末尾。

## 模块与开关

- `crates/codegen/markdown/fuzz/Cargo.toml`
- `crates/codegen/markdown/fuzz/README.md`
- `crates/codegen/markdown/fuzz/fuzz_targets/render_all.rs`
- `crates/codegen/markdown/fuzz/seeds/render_all/bench.md`
- `crates/codegen/markdown/fuzz/seeds/render_all/code_block.txt`
- `crates/codegen/markdown/fuzz/seeds/render_all/inline.txt`
- `crates/codegen/markdown/fuzz/seeds/render_all/lists.txt`
- `crates/codegen/markdown/fuzz/seeds/render_all/math.md`
- `crates/codegen/markdown/fuzz/seeds/render_all/mixed.txt`
- `crates/codegen/markdown/fuzz/seeds/render_all/table.txt`
- `crates/codegen/markdown/fuzz/seeds/render_all/thematic_emoji.txt`
- `crates/codegen/markdown/fuzz/seeds/render_all/unicode.txt`

Cargo feature：`{}`。

## 功能与规范映射

- [Markdown fuzz package boundary](../specs/developer-support/spec.md#requirement-markdown-fuzz-package-boundary)：markdown-fuzz SHALL 作为独立 nested workspace、不可 publish 的 edition 2021 包提供 libFuzzer render_all target，引用父 markdown；target 禁用 Cargo test/doc/bench。
- [Markdown fuzz chunking and oracle](../specs/developer-support/spec.md#requirement-markdown-fuzz-chunking-and-oracle)：render_all SHALL 拒绝非 UTF-8 输入；streaming 轮换 1/16/32 字节目标长度并向后对齐字符边界，每块 push_and_render。
- [Markdown fuzz seed corpus](../specs/developer-support/spec.md#requirement-markdown-fuzz-seed-corpus)：fuzz seeds SHALL 保留 9 份手工文本，覆盖 code、inline、list、mixed nesting、table、math、Unicode、thematic emoji 和较长 benchmark 文本。

## 边界

- 不会据此运行这个独立 fuzz target。
- 仅执行 pretty true/false × full/streaming 四条 ratatui 路径，Syntect 恒 None。
- 将 end 向后推进至合法边界，不按固定字符数分块。
- 不调用 finish、不比较 full/streaming 输出、不覆盖 ANSI 或 Syntect；README 的八组合描述不是实际覆盖。
- 其中架构与性能陈述只是被渲染文本，不是仓库事实证据。
- 没有执行 cargo-fuzz campaign，不能声称 corpus 已被 libFuzzer 跑过。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 阅读与验证记录

- Cargo.toml、render_all.rs 全 35 行、README 和全部 9 个 seed 已阅读。source_files 从原仅枚举 src 得到的 0 校正为 1，source_lines=35；seed 与 README 另外记录文件哈希，不伪装成 Rust 代码行。
- seeds 包含小型代码、链接图片/强调、heading/列表/task、嵌套引用、表格、数学环境、宽字符与 emoji；bench.md 是较长混合渲染文本，其内代码示例、架构描述、复杂度、预计耗时不作为当前功能事实。
- manifest 和 README 注释称八组合含 Syntect，实际入口固定 None，只有四路径。按 1/16/32 字节轮换而非 char-by-char，UTF-8 end 向后贴边。无 finish、无输出等价 oracle、无 ANSI 调用。没有运行 cargo-fuzz 或种子重放；父 markdown 的 502 单测不能计为本独立包 fuzz 通过。
