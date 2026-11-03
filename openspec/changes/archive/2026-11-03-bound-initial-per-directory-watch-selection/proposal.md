## Why
算法T1：PerDir初始注册遍历所有目录后，收集全部路径并稳定按深度排序，再按watch budget截断。目录远超budget时，未选路径的保留/排序浪费内存和CPU。

## What Changes
把现有budget传入选择阶段，标准库有界BinaryHeap只保留最浅的B个路径，再按原深度/DFS出现次序输出。仍完整遍历ignore walker，统计全部候选数，保持原告警总数。根watch、VCS策略、ignore/pruning、后续Update/Reconcile都不变。

## Impact
纯局部选择算法，不更改可见watch集合/次序与配置；skip_specs。不加后台索引、阈值、缓存或新配置。Tier1差分完整旧选择与有界选择，全fsnotify测试，独立review。
