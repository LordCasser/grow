## Context

viewer 是逐 prompt 的展示归属，internal `subagent_completed` Turn 也会使它为 true。runtime 才能证明一个进程的 root、child 和 draining writer 已达到挂起边界。当前工作树已有统一 `begin_exit_suspension` 和 runtime scope 修复，本次保留该实现并补齐真正触发事故的执行闭环。

## Decisions

- Quit 只发出 `SuspendForExit` 并等待匹配的成功 ACK 后退出。UI 标志不授予跳过 runtime 的权限；重复 Ctrl+D 仍合并，失败不变成取消或强退。已有启动未知、leader 连接 scope 和 writer drain 规则保持原职责。
- 测试使用两个后台 child。B 执行文件门控命令；A 完成后触发 parent 的通知 Turn。parent 已显示该 Turn 的 live 文本且 B 仍未结束时注入两次 Ctrl+D。provider 的终态门控和文件门控固定交错，不靠固定 sleep 猜测。
- root 与 child 使用不同 mock API endpoint，沿已有 expectation matcher 区分并发请求，避免扩大测试 runtime 或新增 matcher。只改变隔离 fixture 的模型路由，不改变产品配置。
- 退出后验证 root/B 的同一挂起 operation、checkpoint 与未结束结果；冷恢复使用原 root、child、Turn 和 Spawn，B 只有一个完成结果，A 不重跑。命令中断是所属任务的持久证据，恢复不自动重发原命令。
- 沿用现有 mock、PTY 与 Timeline testkit，普通/minimal 共用 fixture。新回归进入 core-regression；真实用户会话不参与测试。

## Verification

先运行已有 UI 回归与新 PTY fixture；使用仓库指定 Rust toolchain 构建默认 CLI，对同次产物执行普通/minimal 新场景和既有递归树恢复回归。检查首个 Ctrl+D 无挂起、第二个确认退出，无 cancelled child/Turn、无重复输入/Spawn/命令及结果链接。执行格式、diff、OpenSpec strict 和对抗性审查后归档。
