## Why

恢复历史窗口验收中发现：viewport顶端落在entry后的gap行时，capture把gap归到前一entry，restore却夹到最后内容行，导致同几何往返偏移。本轮用户要求发布前闭合当前遗留，因此由Scrollback布局owner单独修复。

## What Changes

保持gap相对前一entry末尾的位置，补页或重排布局后仍指向该间隙；保留正文逻辑行在宽度变化时的夹紧语义。复用现有ScrollAnchor，不新增持久化状态或历史协议。

## Impact

Pager Scrollback布局与锚点回归；文档链接到client-surfaces契约。
