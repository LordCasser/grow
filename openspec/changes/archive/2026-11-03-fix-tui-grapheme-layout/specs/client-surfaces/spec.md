## ADDED Requirements

### Requirement: Shared TUI text layout preserves grapheme boundaries

Pager 与 Minimal 共用的内置文本换行、字符串宽度截断和搜索显示列映射 SHALL 使用与 Ratatui 相同的完整字素显示宽度。自动拆分长词及截断 SHALL 保留完整字素；原有分词、样式、软折行 joiner 与源文本关系 SHALL 保留。单个字素比可用列数更宽时 SHALL 保留为不可分的片段，由现有绘制边界裁剪。本要求覆盖字素完整位于文本 span 内的输入，不改变跨 span 的字素归属、textwrap 自定义 callback、终端的模糊宽度策略或字体。

#### Scenario: Mixed Chinese and composed symbols wrap
- **WHEN** 中文和英文中混有变体选择符 emoji、ZWJ emoji 或组合音标并在窄列宽换行
- **THEN** 可以容纳的输出行不超出可用显示列数，换行不拆开字素，按 joiner 重建可恢复源文本。

#### Scenario: Truncation preserves a complete symbol
- **WHEN** 共享字符串截断的宽度边界落在多码点字素内
- **THEN** 保留完整前缀字素并预留省略号宽度，结果不超过预算且不残留孤立的变体选择符或 ZWJ。

#### Scenario: Search follows a composed symbol
- **WHEN** 搜索匹配位于组合符号之后
- **THEN** 高亮的显示列与实际绘制列一致，搜索字节范围换行与正文使用相同断点。

证据入口：`crates/codegen/pager-render/src/render/wrapping.rs` 的 `wrap_ranges_trim`、`word_wrap_line_with_joiners`、`byte_offset_to_display_col`；`crates/codegen/pager-render/src/render/line_utils.rs` 的 `byte_offset_at_width`、`truncate_str` 及回归测试。
