## ADDED Requirements

### Requirement: Minimal transcript snapshots write outside the interactive loop
Minimal 全文 transcript 的渲染 SHALL 继续按每帧 8ms 预算分片；渲染完成后的 ANSI 临时快照文件 I/O SHALL 在后台阻塞任务执行，避免最终写入占用 UI 事件循环。

#### Scenario: Snapshot write is slow
- **WHEN** Minimal transcript 的完整 ANSI 正文已渲染且快照写入尚未完成
- **THEN** UI 事件循环继续处理输入和绘制；分页器请求只在完整私有快照写入成功后提交。

#### Scenario: A newer request replaces an in-flight write
- **WHEN** 旧 transcript 快照仍在后台写入时产生更新的 Minimal transcript 请求
- **THEN** 旧结果完成后被释放，不启动旧分页器；仅当前请求的完整快照可交给分页器。

#### Scenario: Original owner disappears or changes binding
- **WHEN** 快照写入完成时原 root、选中 child view 或 session identity 已不存在或不匹配
- **THEN** 丢弃并清理该快照，不将文件或错误反馈交给当前其他会话。

#### Scenario: Snapshot write fails
- **WHEN** 后台创建或写入 ANSI 快照失败
- **THEN** 清理未完成的私有临时文件，并将错误反馈给仍匹配的原 owner；UI 事件循环不等待磁盘操作。
