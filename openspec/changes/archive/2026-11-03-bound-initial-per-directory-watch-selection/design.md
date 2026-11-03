## 不变量
pruning_walker继续决定候选；root排除、symlink不跟随、git/custom globs全保留。每个候选的(depth, source ordinal)为总序，完全等价原稳定depth sort，同深度不按路径字典重排。heap保存当前最小B项，最大键在顶；只在更小候选到来时替换。B0仍遍历/计数但无路径保留；B>=N输出全部。不得用budget预分配任意大capacity。最终最多B项排序。

返回选集和完整候选数，用于既有预算告警；不减少目录IO，不改变后续watch失败处理。walker产出的路径深度仍用原components().count()保证等价。

## 验证
把同一候选序列喂给旧stable sort/truncate和新选择，N0/1/128/512/2048/12000，B0/1/8/128/N/MAX，单深度与混合深度/相同键次序，比较精确路径顺序和总数。保留真实filesystem ignore/symlink/pruning测试，并用其结果核对预算裁剪。临时成本探针含实际walker全遍历和纯选择阶段，setup在计时外，2warmups+7samples；分别报告选集保留上界和耗时，不把省内存说成少扫描IO或更少内核watch。
