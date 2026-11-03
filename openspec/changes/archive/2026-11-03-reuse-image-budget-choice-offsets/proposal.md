## Why
算法C1：同一图片预算选择每次构建先按SurfaceId线性定位验证，再线性定位应用；effective parts另扫完整Surface。现有cache已保存完整surface_ids前缀，eviction已给出item/part位置，可直接复用该绑定，不必新增索引。

## What Changes
replaced私有key改为item/part offset，以saved/current同位置SurfaceId与原Weak身份核对。域/prefix重置、builder任一失效全清、pressure逐项跳过、对外evicted_parts顺序和数据保持不变。

## Impact
仅ChatState非持久、无IPC的私有cache表示与局部访问优化；不增加状态字段/索引、不改变cache准入和状态迁移、错误/存储/产品契约，skip_specs。
