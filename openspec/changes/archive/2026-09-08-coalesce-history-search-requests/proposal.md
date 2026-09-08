# Why
历史搜索UI和Drop向容量256的通道阻塞send，worker忙于匹配时可能卡住输入或关闭。audit-prompt-history-and-draft-entrypoints的H1已确认调用闭环。

# What Changes
将待处理请求合并为最新items/query，使用有界非阻塞唤醒；关闭不等待队列空间。保留SetItems重置query、随后SetQuery组合到同批items的语义。

# Impact
仅history_search的调度。H2请求身份/旧快照与D1草稿读取分开修复；不删R21、不改匹配/排序/输入历史内容。
