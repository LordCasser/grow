## Boundary

`collect_input_batch` 可在一次 drain 内收集多个独立事件；它的收集窗口只控制公平性，不提供粘贴归属。Crossterm 0.29 的 Unix parser 在 `ESC[201~` 结束标记到达前返回 `None`，之后才生成携带完整正文的 `Event::Paste`。Windows 输入解析器不生成 `Event::Paste`；其 unbracketed 路径仍由快速按键检测处理。

因此 `coalesce_rapid_keys` 只在连续 `Key` run 内推断未 bracketed 粘贴。遇到 `Event::Paste` 时按原顺序直接保留，作为相邻 key run 的分隔符。移除把批内所有 paste、字符和 Enter 拼成一段的 `merge_paste_fragments`；不再根据批次相邻关系丢弃非文本按键。

## Verification

在同一批次注入完整 paste 后 Enter、字符、方向键、Ctrl+C 及第二个 paste，要求事件顺序和内容逐项保持；既有未 bracketed 大粘贴检测仍通过。对本 crate 的事件循环测试运行回归。
