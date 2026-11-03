## Context

动机见 [proposal.md](proposal.md)。本次用户确认退出操作是连续两次 Ctrl+D。Timeline 是终态和挂起事实的权威；统一日志用来关联进程与 shutdown，memtrace 用来核对进程起始时间。截图中的任务组没有取消发生时间，不能单独作为 resume 取消的证据。

## Decisions

- 只读核对用户指定 root 和五个 child，不启动真实会话或调用真实 provider。保存时间、身份、事件 seq、终态来源及错误类别，不复制任务正文或配置。
- 当前 `router::begin_exit_suspension` 在 Quit 时发出 `SuspendForExit`；`ExitSuspensionResolved` 只有成功才发出 Quit，失败保持 Unconfirmed。runtime owner 范围修复已在 `2026-11-02-scope-exit-suspension-to-runtime-owners` 归档，不重复实施。
- 使用现有 `suspend_nested_task_tree` PTY 场景，覆盖 Ctrl+D、原 child/grandchild checkpoint、新进程原身份恢复以及原操作不重放。运行对象明确为用户当前安装的二进制，不能把此结果称为本次完整工作树构建通过。
- 事故中的 child 已有 cancelled 终态，没有对应的未消费挂起 checkpoint。普通 root resume 保留这些终态；不删改历史、不自动把终态恢复成运行。显式 Task resume 是已有的派生执行入口，不能冒充原树 checkpoint 恢复。

## Evidence limits

旧进程在退出挂起范围修复之前已启动，退出时没有提交新的挂起事实，并实际进入 shutdown。旧进程没有记录退出 UI 分支和构建内容摘要，不能仅凭相同版本号、文件 mtime 或进程起始时间断言具体绕过分支。当前回归与事故事实分别记录。
