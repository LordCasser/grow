## Context

范围来自用户追加要求：整个 Grow 代码仓、各个版本，不局限当前特性。当前 Cargo metadata 列出 57 个 workspace package；另有 vendor、fuzz、发布/验证脚本与配置。Git 非 shallow，本地发布标签从 v1.0.0 到 v2.6.0，共 46 个。`openspec/specs` 是当前契约；历史 change/旧设计仅作为当时意图与证据，必须核对对应版本代码。

## Goals / Non-Goals

产出可复核的全仓结构和关键执行链审查，覆盖每个模块及每个标签的相关变化。审计范围包含目录/模块聚类、接口与状态所有者、错误/取消/持久化/授权边界、无当前需求支撑的抽象或兼容路径及验证缺口。风格偏好、文件大小和简单关键字统计不单独构成缺陷。历史缺陷已修复时明确修复版本，不将旧问题列为当前阻塞。审视不授权全仓重构或修改历史发布代码。

## Decisions

### Partition by responsibility

分区互不写业务代码，各自记录唯一报告。当前分区：

1. Session orchestration 与持久化：shell agent/session、chat-state、prompt-queue、workflow（模型控制专题已有独立 reviewer，结果纳入此区）。
2. Sampling 与模型数据：sampler、sampling-types、compaction、token-estimation、grow-http、auth。
3. Tools 与权限/隔离：tools、common tool-{protocol,runtime,types}、workspace permission、sandbox、hooks、mcp、workspace-types。
4. Agent/config/extensions：agent、config、config-types、plugin-marketplace、extension-types、announcements、shell config/extensions。
5. Workspace 与存储/索引：workspace 非 permission、fast-worktree、grow-gix-status、hunk-tracker、codebase-graph、fsnotify、memory、sqlite-journal、paths。
6. Client/UI/terminal：pager、pager-minimal、pager-render、markdown、mermaid、ratatui-inline、ratatui-textarea、client-support、acp-transport、shell-base。
7. Product/build/platform/quality：cli、update、version、diagnostics、crash-handler、tracing-macros、tty-utils、ptyctl、ptyctl-cli、pager-pty-harness、test-support、test-utils、scripts、CI、vendor、fuzz、根目录资产和文档契约一致性。

审查公共边界时可读相邻区，但报告明确依赖事实，由架构 owner 去重、核实共享决策和重测活动功能。每区依次检查当前入口/调用方/测试；逐标签用 tree/hash、manifest/目录与相邻 diff 判断变更，并深入有架构意义的变化。完全相同树可复用前一版本证据，必须显式标注。

### Evidence and findings

每个发现包含版本与位置、可达触发、受损不变量、失败后果、现有边界为何覆盖不到、修复所有者和可证伪通过标准。当前缺陷优先必要的定向实验；静态证据足够时标注静态结论。不能把测试缺失本身推断成运行失败，也不能为设想攻击新增运行时防护。分区未覆盖/无法运行的范围写明，不能宣称零缺陷或全局最优。

### Delivery and debt

所有新过程记录位于本 change；无行为 delta。总报告归纳横向依赖、状态与一致性问题，链接分区事实。与当前 model/effort 功能直接相关的问题进入其 change；其他现存债务写 backlog，包含证据、影响、独立修复边界。审计完成需模块×版本覆盖可解释、发现已核实分类、文档链接与 OpenSpec strict/archive 校验通过。审计不要求修复所有发现。

## Risks / Trade-offs

- 仅检查 HEAD 会漏掉历史旁路 → 每区必须提供全部标签覆盖表和树/diff依据。
- 标签历史存在大规模迁移 → 从旧 tree/module path定位，不以现在路径缺失当作没有功能。
- 多 reviewer 重复或互相矛盾 → 父 agent保留架构合成与发现验收，交叉核实关键调用链。
- 活动实现使审计基线变化 → 单独标注 HEAD/初始工作树/最终特性，受影响区域末尾复核。
