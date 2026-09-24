## Why

Pager 将同一输入批中的 `Event::Paste` 与随后普通按键合并，甚至丢弃方向键或 Ctrl+C。Crossterm 的 bracketed paste 事件已在结束标记到达后包含完整正文；批次相邻不代表同一次粘贴。结果是粘贴后立即提交、继续输入或取消的操作可能被吞掉。

## What Changes

- 将完成的 `Event::Paste` 作为独立输入边界，保留后续按键及第二次粘贴的原始顺序。
- 继续只对未 bracketed 的连续快速按键做既有多行粘贴检测。
- 用同批次 paste 后的 Enter、字符、控制/方向键和连续两次 paste 覆盖回归。

## Impact

仅更改 Pager 输入合并；不改变粘贴正文解析、文件/图片分类或其他输入协议。
