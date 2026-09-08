# Findings

## Live entrypoints
/history在builtin registry注册，run发出OpenHistorySearch；Up路径在agent_view/prompt.rs进入activate_browse。PromptHistoryLoaded会refresh_items并在搜索模式重发当前query。root的后台完成处理对agent和child调用history_search.poll。Enter/Tab与mouse接受路径使用selected_text。

LocalDraftRuntime在event_loop的sync_local_drafts以及退出flush入口运行；load用于session或cwd键恢复，恢复仅填本地composer和deferred behavior，不重新发送ACP。

## H1: Blocking history queue
views/history_search.rs Daemon::new构造sync_channel(256)，refresh_items/update_query以及Daemon::drop都使用send。worker串行进行build_items和全历史匹配/排序；当worker尚未接收而第257条消息到达时，标准库通道会阻塞发送者。注释“never blocks”与实现冲突。drain_to_latest仅在worker取到消息后执行，不能防止UI先阻塞。修复应保留最新items+query的组合语义并让停止不等待容量，不可简单try_send后丢弃最新query。

## H2: Old snapshot accepted on reopen
activate_inner先refresh_items，再直接复制daemon.shared的已有Snapshot，selected立即指向其最后一项；deactivate仅清空UI snapshot，未清空shared。新工作完成前Enter/Tab可选取旧搜索结果。generation仅为worker完成计数，UI没有对应当前请求的identity；poll也不能拒绝已过期但刚完成的旧请求。后续应为请求/结果建立身份并验证重开、快速输入和晚到刷新，不仅清空显示一次。

## D1: Draft source boundaries
LocalDraftStore::load用File::open，未检查regular file且Unix无O_NONBLOCK，FIFO可在UI同步恢复路径等待writer。metadata之后read最多MAX_RECORD_BYTES+1，但未检查bytes.len；当文件增长且前MAX+1字节仍为合法JSON（例如尾部空白），超预算内容可能通过parse/validate。已有元数据超限/坏JSON隔离政策要在独立修复中保留。测试应覆盖实际增长、精确边界、FIFO与目录，不能仅测试静态超大文件。

## R21
HistorySearchState::selected()仅返回None。全仓Rust引用检索未找到HistorySearchState::selected或history_search.selected调用；selected_text是实际接受入口。候选仅该空壳方法和专属注释，保留HistoryEntry输入结构和真实导航/渲染。公共仓库外消费者未知。
