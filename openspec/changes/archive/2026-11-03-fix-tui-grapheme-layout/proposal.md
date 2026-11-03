## Why

用户截图显示中英混排和圈号附近存在字形重叠。核对共享渲染发现，换行、宽度截断和搜索列映射仍按 Unicode scalar 累加，而 Ratatui 按字素绘制；变体选择符、组合 emoji 会使两者宽度和边界不一致。截图圈号的字体/终端模糊宽度差异尚未复现，不将其归因为已确认的代码缺陷。

## What Changes

- 共享换行使用与 Ratatui 一致的字素宽度，长词只能在完整字素之间拆分。
- 共享字符串截断及搜索列映射按字素计算，保留中英混排、样式和软折行原文。
- 增加旧实现失败的回归，覆盖中文、变体选择符、ZWJ、组合音标及窄宽度。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `client-surfaces`：明确共享文本布局的字素与显示列一致性。

## Impact

修改 `pager-render::render::{wrapping,line_utils}`；普通 Pager 和 Minimal 的调用方沿用原入口。复用现有 textwrap 的 Unicode 分词和 Fragment 排版 API、unicode-segmentation 与 unicode-width，不增加依赖、配置、持久化状态或终端宽度探测。不承诺修复字体墨迹越界或终端采用不同 ambiguous-width policy 的情况。
