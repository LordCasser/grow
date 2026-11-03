## Why
算法 T2：一次 GetFileHunkData 会对同一文件的每个 hunk 重新扫描 baseline/current 字符串、分配两份行表。文件接近既有 1 MiB 上限且 hunk 多时，重复工作随 hunk 数增长。

## What Changes
仅在一次文件查询内构造 baseline/current 的借用行表并复用；原单 hunk 入口与批量查询共用现有 patch 格式化算法。空/新增/删除/Binary/TooLarge/Missing 状态、上下文、header、换行和完整返回值不变。不加跨查询缓存。

## Impact
纯局部计算复用，不改变行为/接口/持久化契约；skip_specs。先做旧算法字节差分和多 hunk 规模基线，完成后全 hunk-tracker 单测与独立 review。
