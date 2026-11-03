## Context

共享文本由 `Line` 展平，经 textwrap 获取 byte ranges，再按原 spans 回填样式和 joiner；搜索复用同一 ranges。textwrap 0.16.2 的 `display_width` 和 `Word::break_apart` 按 scalar 计宽/拆分，与 Ratatui 的字素宽度不同。已有 CJK 单字符测试通过，不能覆盖 `⚠️` 从 1 列变 2 列以及 ZWJ 序列合为 2 列的情况。

## Goals / Non-Goals

在现有共享所有者修正宽度和拆分单位，不改变会话、输入、终端重绘或原生提交状态。截图中的圈号是否涉及 fallback 字体或 ambiguous-width 设置单独保留不确定性，不凭截图全局切换 CJK 宽度。

## Decisions

复用 textwrap 的 Unicode word separator、hyphen splitter 和通用 Fragment 算法，用局部 Fragment 携带字素宽度；超过预算的词按 grapheme_indices 拆分。继续由原始 byte ranges 驱动样式、搜索、joiner，避免二次重排掩盖超宽行。内置 FirstFit/OptimalFit 使用新计宽；保留 textwrap 自定义 callback 的原 Word API 和计宽语义，目前没有该选项的生产调用方，不为它引入新接口。

字符串截断先扣省略号预算，再定位完整字素边界；显示列按完整字素累加。全部为纯计算，无新增状态和失败恢复路径。

## Risks / Trade-offs

- 断点变化可能影响选择/链接 → 验证 byte ranges、joiner 重建、样式和既有 OSC8 测试。
- 字素大于预算无法拆分 → 保留不可分片段，沿用绘制裁剪，不造替代字符。
- 终端/字体宽度与 Unicode 默认不同 → 明确边界，不增加猜测性的宽度检测或开关。
