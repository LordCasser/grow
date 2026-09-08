# Delta

## ADDED Requirements

### Requirement: Web fetch cache respects configured capacity
web_fetch 文本缓存 SHALL 遵守 max_cache_entries 最大条目数，零表示不保存缓存；更新已有 URL SHALL 不淘汰其他条目。

#### Scenario: 零容量
- **WHEN** max_cache_entries 为零且获取完成的文本尝试进入缓存
- **THEN** 不保留条目，后续缓存查询不命中。

#### Scenario: 满容量更新
- **WHEN** 缓存已满且再次写入已有 URL
- **THEN** 更新该 URL 内容与插入时间，保留其他条目。

#### Scenario: 满容量新增
- **WHEN** 缓存已满且写入新 URL
- **THEN** 淘汰最早插入的条目，插入新内容且总条目数不超过配置上限。
