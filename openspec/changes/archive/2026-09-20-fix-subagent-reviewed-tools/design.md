## Context

见 `proposal.md`。当前系统已经有 authored exact identity、initial RWX、PermissionManager、primary-context Auto judgment 和 one-shot permit。问题是 descriptor 只能声明最大 RWX，无法表达“即使 RWX 覆盖也必须审核”；权限管理器又从 `AccessKind::Edit(path)` 反推工具名，使 `write` 丢失身份。

## Goals / Non-Goals

**Goals:**

- 用一个 descriptor-owned policy 表达工具的逐次子 Agent 审核语义。
- 让 runtime 注入的 `write` 仅以 approval-required 身份进入 child eligibility。
- 让审核请求、审计与执行 permit 指向同一个冻结调用。
- 复用现有 PermissionManager mode、主上下文 Sideband 和 dispatch revalidation。

**Non-Goals:**

- 不增加可变 session grant、路径租约或第二套主子通信协议。
- 不改变 primary Agent 的 `write` 行为。
- 不在本 change 修复文件系统 read→write 竞争或跨进程 CAS。
- 不把 `ToolKind` 变成授权来源。

## Decisions

### 1. `ToolCapabilities` 声明 child review policy

增加闭合枚举，默认沿用现有 authored/RWX 行为，显式 `Required` 表示该工具在 child 中总要经过 PermissionManager。最终 registry/bridge 将 policy 与 max_access 一起投影；`ToolKind` 继续只负责展示。

`Required` 同时承担两个必要语义：实现若在最终 bridge 中存活，即可作为 locked identity 进入 hard eligibility；`native_call_available` 始终返回 false。这样 runtime injection 不会获得免审权限，也无需把它伪装进 authored preset。

备选方案“把 write 加进 grow-build preset”无法阻止 ReadWrite/All fast path；“按 ToolKind::Write 特判”会让展示 taxonomy 反向参与授权；新增 Agent frontmatter allowlist 会把工具固有用途重复到每个 Agent。均不采用。

### 2. 能力状态保留 descriptor policy

`SubagentCapabilityState` 的 visible/eligible native entry 同时保存 max_access 与 child review policy。目录分别渲染 available、call-projected、approval-required 和 forbidden。approval-required 指向精确调用 Gate；visible forbidden 明确该 child 不可获批，并让真实依赖通过 `ask_parent` 报告给父级处理或重派，父级回复本身仍不授予权限。Agent switch 重建同一投影并提升 epoch；dispatch 继续复验 exact eligibility。

### 3. Shell 产生冻结调用证据，PermissionManager 不再猜 exact name

preflight 已持有 finalized wire name、解析后的 typed input 和 canonical JSON。它构造 `PermissionCallEvidence`：exact name、canonical argument hash，以及可选的有界 operation summary。

`write` summary 使用 `replace_entire_file` discriminator、模型路径、UTF-8 byte length、BLAKE3 digest 和字符安全截断的 preview。summary 是不可信操作证据，不建立用户授权；主上下文 System policy保持这一边界。其他工具仍至少携带 exact name/hash，既有 access detail 用于权限规则和 UI。

PermissionManager 的 classifier 和 audit 优先使用 evidence 的 exact name；没有 evidence 的旧内部调用沿用 access-derived name，避免无关测试 helper 和非 Shell 调用被迫伪造 identity。

### 4. 现有 permit 继续是唯一执行 authority

permission allow 之后，Shell 从同一 parsed args 签发 permit；dispatch 继续核对 tool name、dispatch target、canonical args、cwd、RWX、actor epoch 和 MCP generation。review policy不产生持久化授权，也不修改 child state。

### 5. Permission mode 保持单一所有者

review-required 只关闭 initial-RWX fast path，不绕过显式 deny、managed Ask、protected edit 或 operator 选择的 child permission mode。Auto 使用 primary-context judgment；Ask 使用现有交互；AlwaysApprove 保留显式配置语义。目录使用“permission Gate”而不是承诺每个 mode 都调用主模型。

## Risks / Trade-offs

- [descriptor policy 被错误标记会扩大 hard eligibility] → 仅具体工具实现可以声明，默认不扩大；最终 bridge/policy filtering 和 dispatch eligibility 仍必须同时成立。
- [write preview 可能很大或包含提示注入] → 字符安全固定预算、保留总字节数和 digest，并继续放在明确的 untrusted payload 中。
- [audit 名称变化影响既有断言] → 只在存在 frozen evidence 时使用 exact name，增加 Write 与 SearchReplace 分离回归。
- [AlwaysApprove 不进行模型判断] → 这是既有显式 operator mode；规范把保证定义为进入配置的 Gate，而不是无条件调用主模型。

## Migration Plan

直接修改内存 descriptor 与权限请求；没有持久化格式迁移。回滚时移除 descriptor policy/evidence 字段及 `write` 声明即可恢复旧行为。
