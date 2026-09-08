# Why
capture_agent拒绝composer和staged_prompt同时存在，但validate_record只验证各字段，加载可接受两份输入并由restore_agent静默优先staged，丢弃composer。

# What Changes
共享记录校验增加二者互斥约束，异常写入拒绝且保留原文件，异常磁盘记录沿用隔离策略。

# Impact
不扩展恢复排序，不支持新的多草稿功能；单composer、单staged及仅Behavior的正常记录保持。
