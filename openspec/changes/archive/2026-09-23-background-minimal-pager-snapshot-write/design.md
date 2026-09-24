## Evidence and existing boundaries

- `pager-minimal::full_view::pump_transcript` 在 draw hook 内逐块渲染，每片使用 `PUMP_BUDGET = 8ms`。
- 片段完成后 `finish_transcript` 调用 `minimal_api::app_set_pending_pager_for_transcript`，其内部 `pending_pager_for_agent` 同步调用 `write_pager_transcript`；此时仍在同一 UI/draw 线程。
- 1/10/100 MiB 的 Markdown render 中位数为 0.046/0.368/3.541 ms；快照写入为 0.739/2.622/42.191 ms，100 MiB 最高样本 142.095 ms。測量详见 `verification.md`。
- Full/TUI 同步 Markdown 拼接和写入也在 dispatch 线程，但归档契约没有对应的全局 100ms 响应时限；输入 fairness 规范只限定多行粘贴每轮事件预算。因此不以本次数据为 Full/TUI 新契约依据。

## Decision

复用 Pager 已有 `Effect`/`JoinSet<TaskResult>` 将 Minimal 快照写入放入 `spawn_blocking`。Minimal draw hook 在 loop 尾端生成快照 Effect 后，event loop 立即 drain 并启动 worker，不依赖后续输入或 ACP 通知唤醒。已渲染的 ANSI `String` 与最小 owner identity 可跨线程移动；scrollback/RenderBlock/highlighter 不跨线程。

Minimal transcript 请求持有单调代次。新请求递增代次并替代尚未完成的渲染请求；后台旧写入无法被强制中断，但完成后若代次过期即丢弃其 `TempPath`，不打开过期内容。完成结果同时校验 root agent、选中 child key、view agent identity 和 session binding；owner 不存在/已换绑时释放临时文件，不对后来会话写反馈。

成功结果把 worker 创建的私有 `TempPath` 直接转为 `PendingPager`，避免在 UI 线程二次创建或同步写入。失败只向仍匹配的原 owner 追加错误 notice。应用退出时正在运行的操作不承诺可强制停止；临时文件所有权仍由请求/结果 RAII 清理。

## Verification

使用受控阻塞 writer 验证 current-thread Tokio runtime 可在磁盘写入等待期间继续消费已排队输入事件；验证 worker 成功快照完整、权限为 Unix 0600、失败清理临时文件，以及过期代次/失效 owner 的结果不能 arm pager 或污染其他会话。
