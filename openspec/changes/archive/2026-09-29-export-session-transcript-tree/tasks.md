## 1. 固定来源与委派层级

- [x] 1.1 实施前复核 analysis 的真实入口、现有工作树及主规范；用实际 root-owned nested spawn/seed fixture 证明 lifecycle owner 与直接父身份不同，并记录证据。
- [x] 1.2 在现有 storage 边界实现只读 transcript 来源与必要 owner 索引；验证 root/child/grandchild、指定 child 为根、跨 cwd/worktree、fork/resume 来源排除、循环/重复/冲突/缺失 child 的 fixture。
- [x] 1.3 复用 rewind 和 response reconciliation，保留 ACP/Grow envelope、meta、来源顺序与原始时间依据；验证缺失 canonical cache 补全、discard/quarantine/rewind 排除及独立事件交错，不以合成当前时间冒充历史时间。
- [x] 1.4 固定读取边界并执行全树预算；验证活跃 writer 共存、前件尚未落盘的可见失败、源文件前后哈希一致及超限停止。

## 2. 完整会话展示与 CLI 输出

- [x] 2.1 按 design 的输入覆盖矩阵提取最小纯展示 helper，复用 tracker/blocks 和必要 store；以 live 与 offline fixture 比较 display text、hidden echo、thinking、工具结果、Grow notice/Hook/子 agent/后台任务和 Goal/Workflow 展示。
- [x] 2.2 新增完整 Markdown 序列化，保留原紧凑 TUI renderer；验证多字节文本、Markdown fence、结果截断、附件说明、合法空节点、child 链接和正文不跨 session 混入。
- [x] 2.3 将 CLI 参数改为目录并删除 clipboard 路径；验证默认 cwd/session-id、显式相对/绝对/~ 路径、`--clipboard`/`-c` 拒绝，以及无需模型配置即可进入导出。
- [x] 2.4 实现私有临时树与 no-replace 发布；测试已有文件/空目录/非空目录/符号链接、发布竞争、子节点中途失败、输出超限与临时资源清理，成功反馈仅在提交后出现。

## 3. trajectory 下载与回归

- [x] 3.1 在 Pager 组合层注入导出回调，Shell HTTP 只提供固定来源和下载；验证依赖无环，RenderBlock 在 worker 内创建/销毁，不跨线程转移 `!Send` 状态。
- [x] 3.2 添加页面导出按钮和有界 tar.gz 下载；验证过滤器/分页不截断导出、CLI/解包内容一致、token/Host/loopback 检查、并发 busy、取消及响应结束后的产物清理。
- [x] 3.3 更新 `crates/codegen/pager/docs/user-guide/17-sessions.md` 和相关 CLI 帮助，链接本 change 归档后的 client-surfaces 契约，说明重复导出需换目录或由用户处理旧目录；确认 TUI `/export` 文档继续描述当前视图的单份完整 transcript。
- [x] 3.4 运行 `cargo check --locked -p cli` 及相关 shell storage、pager export 与 TUI transcript 回归；保留 `export_file_uses_session_cwd_and_preserves_absolute_targets`、`minimal_child_export_keeps_child_content_path_and_feedback_after_view_switch`、`export_waits_until_history_replay_completes` 和文件队列测试。构建前后检查磁盘，用范围内验证避免无关全量构建，清理本次不再需要的编译产物。
- [x] 3.5 逐项对照 delta 场景，将实际执行命令、结果、限制记入 verification；运行 `openspec validate --all --strict --no-interactive`，实现全部完成后才归档，再执行全量与 archived 校验，并维护后续 replay change 的依赖链接。
