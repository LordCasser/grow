## Why
算法C2：损坏历史中的多个隔离区间、缺失结果插入及重复结果删除仍逐次 splice/remove，分散在前部的K项修复会反复搬移长后缀。健康路径不需要额外重建。

## What Changes
保留原判定算法与修复顺序，仅把已知编辑计划一次合并/retain。隔离证据仍由原始区间生成；缺失结果文案、run局部last-wins和返回report不变。不变更Timeline修复合法性、durable边界及wire格式。

## Impact
纯算法替换，skip_specs；旧算法作为test-only oracle，完整序列化差分、幂等及N/K曲线，执行sampling-types和chat-state相关全量后独立review。
