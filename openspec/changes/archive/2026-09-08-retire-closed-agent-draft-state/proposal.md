# Why
Agent关闭后LocalDraftRuntime.keys/loaded/tracked不清理。同session以新AgentId重开时loaded.insert返回false，跳过load；空composer被捕获后可覆盖/删除原磁盘草稿。长期关闭会话也保留无用序列化文本和路由。

# What Changes
识别最后一个Agent所有者离开的草稿键，检查点保存后释放已落盘运行时状态；保留失败写入的有期限重试，重开时优先恢复尚未落盘的最新内存记录。成功落盘的关闭键再次打开须重新加载磁盘。

# Impact
仅本地草稿生命周期，不删除磁盘草稿或会话。多个Agent共享一个键时不得提前回收。关闭数据不是用户确认删除候选。
