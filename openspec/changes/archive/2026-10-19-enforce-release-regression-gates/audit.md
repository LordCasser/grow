# Release and cross-layer audit

基线：v2.4.0 / cd5d88ec；工作在独立 worktree，未携入主工作区正在进行的 MCP 变更。

## Confirmed gaps

1. inquiry 启动竞态的原修复已归档。actor 单测绕过 runtime/durable admission 的异步接点；真实独立进程测试能发现，但原发布仅构建和 smoke，不依赖完整测试工作流。本次将 native coordination gate 绑到 release 的 immutable commit；不重复实现已修复协议。
2. Windows 导出失败的根因是普通 File::open(directory).sync_all。HTTP handler 的原测试用 mock archive，所以 HTTP 路由绿不能证明 production capture/render/write/tar 成功。独立 `fix-portable-transcript-export` change 修复，并在此 gate 的三平台矩阵运行真实主子会话到 HTTP 解包测试。
3. core 精确引用旧 Ctrl+C 退出测试名，Cargo 选中 0 项仍返回 0。Windows storage 同类精确选择也没有执行数断言。本次统一 exact runner，断言列举唯一、同名实际成功、最终 1 passed/0 failed/0 ignored；保留原 20 项 PTY，追加既有 tree/Skill/Plan 恢复测试。
4. core 只测试部分 crate 的 lib，遗漏 agent/tools/tool-types 测试，也不包含 integration binaries；core/Windows 原本不会在 main push 运行。本次补 package、必要 integration targets 和触发，release 不依赖独立 latest-main workflow 的绿色结果。
5. UI/视觉只测 formatter 会漏 actual wire context；Skill catalog/parser 与 actor publication 分层测试无法单独证明具体模型调用。本轮另两个 change 增加 provider request body 断言与 model tool→durable SkillLoad 回归。

## Existing coverage preserved

Skill commit failure、commit 后 ACK 丢失、prehook cancel、多加载原子组；planning dispatch 的 receive/handled 前缀冷恢复和 Stop 优先级；会话树的根/子/孙、部分 release、并发恢复；Plan review restart 已有测试，加入相应持续 gate 或继续由全 shell lib 执行。避免为每个相邻状态排列增加重复测试。源码不能证明任意真实模型服从提示词，功能测试也不代表实机 Windows 结果；验证记录必须区分本地与 hosted。

## Priority and interactions

P1 导出可用性及 release 不能跳过回归；P1 禁用技能不能被名称回退，加载正文必须经过既有发布门控；P2 视觉请求瘦身与同一操作行展示。changes 分开承载独立契约，测试门禁负责持续覆盖。所有新 fixture 走临时文件/隔离 home，真实服务测试使用 loopback 和受控模型，无新增外部服务凭据要求。

## Additional failure-path debt

实际执行新增 gate 时还发现 Skill PTY 夹具按目录遍历首项选会话，可能选中 Minimal 启动创建的空会话。夹具改为根据唯一持久化 Turn 的输入匹配会话，并为冷恢复失败保留 debug 日志；仍使用真实 `--continue`，保留原 Turn、Skill 快照、命令不重跑和 headless/export 断言。产品 `--continue` 按 cwd 与活动时间选择，不能把夹具问题当作恢复产品缺陷。Plan 恢复夹具还沿用单 Ctrl+C 退出，已按当前双 Ctrl+D 契约纠正；待审批和已延后审批两条真实恢复路径均执行通过。

资源集成测试捕获了 Timeline 参数校验与工具 schema 不一致，已纳入 strengthen-skill-loading 修正及跨层回归。同时发现通用 execute_tool_calls 返回 Err 时被外层匹配丢弃，后续 Step 错误覆盖原始失败；该状态机问题已登记 backlog，需独立 change 处理失败收尾，避免混入资源读取实现。
