# Evidence
root/dispatch/session/modal.rs::remove_agent_and_cleanup调用agents.shift_remove，仅清forked_from和释放allocator缓存。AppView::sync_local_drafts每轮传入当前agents。LocalDraftRuntime::sync插入keys，从不retain/remove旧AgentId；loaded只在rekey时remove新key，关闭不处理。tracked正常flush仅due=None，不释放record和serialized。

重开状态路径：A(session S)第一次sync令loaded含S并保存草稿；移除A后sync没有迭代A且不清loaded；新B(session S)空composer进入sync，loaded.insert(S)==false，无restore；capture空record不同于旧tracked，安排write(empty)->remove(S)。此结论基于源码状态转移，尚未运行复现测试，实施时需真实AppView回归。

# Design constraints
从当前agents路由推导live keys，只有最后所有者离开才能回收。退休时对脏记录做一次检查点；成功清理keys/loaded/tracked，保留磁盘。失败不能丢record，也不能每tick强制重写绕过WRITE_RETRY_BACKOFF；保留due供现有定时重试。退休键后续重开时先恢复仍在内存的最新pending record，不能被旧磁盘副本覆盖或被空composer抹除。成功重试后的孤立state应释放。无需增加通用生命周期框架。

# Validation plan
真实AppView保存->关闭->同session新Agent重开，验证文本/cursor/deferred恢复及未自动发送；循环关闭释放干净keys/loaded/tracked；共享key剩余owner不清；保存失败后关闭保留pending/backoff，恢复存储后重试/重开保留最新草稿。测试只使用临时目录。

# Separate debt
capture失败路径和RPC所有权转移路径先移除tracked，再remove失败仅warn，后者无due重试。其持久化删除语义需另立项，不能混入关闭回收。
