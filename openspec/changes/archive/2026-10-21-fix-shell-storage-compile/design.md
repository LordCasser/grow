保留 storage 层 HistorySnapshot 及 writer load 的 reseed；PersistedInfoLight 的消费方只需要 canonical Timeline 与延迟打开的目录，删除不存在且未消费的 history 初始化，不扩展传递接口。append_journal_fact_sync 由父模块两个既有同步入口调用，使用 pub(super) 可见性，不公开到 crate 外，也不修改写入或锁逻辑。
先保存目标文件快照，以精确替换修改并核对差异；不 reset、stash 或全仓格式化。验证以 cargo check --locked -p cli 通过为准，独立只读复核接口。

实际检查还发现用户消息 Vec<ContentPart> 被当作 Display。使用现有 ConversationItem::text_content 提取文本，沿既有文本投影语义拼接文本部分，不另造序列化或附件展示路径。
