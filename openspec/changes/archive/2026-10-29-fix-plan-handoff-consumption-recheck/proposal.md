## Why
Plan live handoff已由同一Turn消费后再次进入Step边界却误报ACK丢失。

## What Changes
按精确source/hash/revision/kind/submission查询既有receipt对应的durable消费Turn，仅同一当前Turn重复核对成功；其他Turn、Dismissed、缺失/不可用继续拒绝。只查询已有Timeline，不建立第二套消费账本。

## Impact
ChatState查询/投影、Shell控制入口与定向失败路径回归。
