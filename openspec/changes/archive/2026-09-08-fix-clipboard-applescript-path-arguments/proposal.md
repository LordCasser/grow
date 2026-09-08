## Why
读图回退直接将临时路径插入 AppleScript 字符串，写图只转义双引号。路径包含引号、反斜杠或换行时可能解析失败或改变脚本。

## What Changes
三个图片脚本统一用 on run argv 接收路径，由 osascript -- 后的独立参数传入；路径不再参与脚本文本构造。

## Impact
client-support macOS 图片读取/附件读取/图片写入的脚本调用。保留原类型优先与临时目录所有权。
