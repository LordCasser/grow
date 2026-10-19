# Design

复用 ChatStateHandle.runtime_state 观察活跃 runtime；冷/已退役节点仍从既有 load_light 的已验证 Timeline 读取 runtime，避免额外 events().to_vec。在原共享 timeout_at 中执行，不新增取消写入或替代 writer。

只替换原本把 events 绑定为 _ 的两次查询。需要 CanonicalChildFacts 的快照继续完整读取/校验。节点开始、已排空、checkpoint 完成与整树完成记录阶段耗时及身份，不记录 prompt/命令/图片正文。使用可控长历史 fixture 或旧/新查询的本地测量核对分配/耗时差异；不把单机数值当跨平台 SLA。
