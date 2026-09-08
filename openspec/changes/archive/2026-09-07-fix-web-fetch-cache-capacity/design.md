# Design
FetchCache 由 WebFetchClient 的克隆共享，网络请求在缓存锁外执行，因此同 URL 的并发 miss 可重复写入。当前 insert_text 无条件在满容量时先淘汰，更新已有键会错误丢弃另一个页面。使用零容量提前返回与 contains_key 判定，新增键才执行现有最早插入淘汰。排序直接比较 Instant，避免逐项 elapsed 采用不同当前时刻。无需引入 LRU、额外依赖或新缓存实体。

测试直接验证零容量不可命中、满容量替换保留其他页面、新 URL 淘汰最老条目和容量上限；手工设置缓存时间戳实现确定性，不依赖 sleep。
