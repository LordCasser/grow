# Why
web_fetch 的 max_cache_entries 配置为零时仍缓存一条页面，违背最大条目数量的含义。此外满容量时更新已有 URL 会先淘汰其他页面，导致并发请求完成时无故减少有效缓存。

# What Changes
零容量不保存条目；只有新增 URL 且已满时才淘汰最早插入的条目。

# Impact
限制在 FetchCache 容量和替换处理、参数说明及回归；保留 TTL 和非裁剪文本缓存策略。
