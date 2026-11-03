复用既有 retire_agent_view 和 forked_from 来源。只有无真实 binding 且无未发送内容的失败占位页才能自动移除；来源存在且失败页仍 active 时返回来源，先前 attached 时用现有 Dashboard attach 恢复 frame。后台回执只回收自身，不改当前焦点。

pending_first_prompt 仍是未提交用户内容：移到空 composer（非空时保留为未发送 queue）后清除 deferred 自动提交，保留可关闭的失败草稿页；不把其当成已经运行的 fork。清除 MCP startup/replay/prompt-history loading 与 command spinner，显式标题 Fork failed。关闭使用前面已验证的 draft owner 边界。

初始 copy RPC 失败不证明磁盘没有生成副本，因此不执行存储删除；已经绑定的页不接受初始失败回执改写。worktree 创建失败且带 fork 来源时复用同一失败处理；纯 new/worktree 原路径不变。Tier 2，从真实 dispatch fork 构造 placeholder 后送失败回执，覆盖普通/attached/background/draft/绑定替换及 worktree。

Dashboard 对已经 settled 且无 session id 的本地草稿仍经过既有两次确认，再复用 close 的草稿退休边界；不发送永久删除 RPC。正在启动的无 ID 页依旧不得退休。
