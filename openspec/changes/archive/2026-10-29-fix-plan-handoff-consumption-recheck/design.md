## Decision
按精确source/hash/revision/kind/submission查询既有receipt对应的durable消费Turn，仅同一当前Turn重复核对成功；其他Turn、Dismissed、缺失/不可用继续拒绝。只查询已有Timeline，不建立第二套消费账本。

## Verification
先构造生产入口反例，再验证成功/错误/恢复与重复操作；持久化事实不靠内存pending缺失推断。
