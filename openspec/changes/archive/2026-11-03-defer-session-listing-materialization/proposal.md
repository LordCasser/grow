## Why
算法S3：列表先将全部Summary格式化为行，再把时间字符串parse回日期排序；大量最终会丢弃的行还会查worktree label。可在已有typed Summary上完成query→时间/ID稳定排序→空会话cwd去重→limit，再转换选择行。

## What Changes
推迟行构建并使用已有UTC typed时间排序。保留当前last_active优先、updated fallback、同时间ID次序及相同key输入稳定次序，空cwd normalize/query/limit/返回字段不变。不建索引/缓存/分页新框架。

## Impact
仅局部计算顺序/分配优化，不改变列表契约或数据，skip_specs。先建立旧算法差分与有界benchmark基线，实施后同fixture测量，不把合成pure函数收益声称为端到端搜索/磁盘性能。
