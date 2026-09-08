# Why
LocalDraftRuntime在capture错误和ACP prompt RPC所有权转移时先移除tracked，再尝试remove。失败只warn，不进入next_deadline/flush_due，旧磁盘草稿可能遗留。关闭回收修复只保存待写记录，不覆盖删除意图。

# What Changes
保留逐键删除意图和重试期限，失败退避重试；重开不得恢复仍待删除的旧内容。新有效非空草稿取消该键旧删除意图。cwd/session两个身份的清理独立追踪，不能因rekey丢失旧键删除。

# Impact
仅本地恢复无效化，不改变ACP发送成功标准，不阻塞RPC等删除成功，不增加持久化journal或自动重发。进程崩溃且文件系统一直拒绝删除时不能保证跨重启无残留，必须明确限制。
