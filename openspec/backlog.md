# 未解决边界

截至 2026-10-20，已确认的 backlog 缺陷已分别立项、补齐回归并修复；完成记录见下表。以下仅保留用户此前明确暂缓的方向：Remote Control，以及依赖真实路由与负载证据的 Provider/Recap 缓存优化。MCP 不在本轮修改范围。

- **Remote Control / Agent Host 跨设备使用（暂缓、未排期）**：用户认可从其他设备连接和操作已有 Grow 会话的方向，但当前不实施、不设日期，不纳入 MCP 多轮或保留计划提案。本次不展开远端控制架构，也不因此引入统一 Dock、Memory 或其他大型功能。未来明确启动时另建 change，重新界定身份、权限、执行 owner、断线和恢复边界；本条不构成实现授权。决策见 [2026-10-11 上游主题评审](changes/archive/2026-10-18-record-upstream-review-2bdd1d6/review.md)。

- **Provider 缓存策略与 native epoch 的关系（暂缓、待真实路由证据）**：[理论 review](changes/archive/2026-09-28-evaluate-provider-cache-affinity/results.md) 只能确认当前 key、breakpoint 与 retention 的本地机制及各官方契约，不能证明统一调整会改善多 provider 的成本与延迟。若要优化，先确认具体 provider/model/deployment、字段透传与负载，再按该 route 的真实证据另开最小行为 change；不预建通用 cache policy registry。
- **Recap 与主请求缓存的关系（暂缓、待真实共享前缀与收益证据）**：recap 使用独立 key 且没有主请求 tools，已修正把本地 wire 形状表述成远端复用的注释和测试名。真实共享前缀与收益仍未验证；保留 Sideband tool-free 契约，不能为缓存假设增加工具定义。

## 本轮已解决

| 问题 | 归档与验证 |
| --- | --- |
| 持久 Bash 捕获前局部变量遮蔽同名导出值 | [捕获隔离修复](changes/archive/2026-10-20-fix-bash-env-export-capture-shadow/verification.md) |
| session 移除/空闲卸载后动态配置监听未回收 | [resident 监听生命周期](changes/archive/2026-10-20-reclaim-session-config-watches/verification.md) |
| 精确测试 runner 缺少阶段进度、子进程期限与有界收尾 | [runner 加固](changes/archive/2026-10-20-harden-exact-cargo-runner/verification.md) |
| sqlite-vec metadata shadow 名称在公共析构遗漏释放 | [名称生命周期修复](changes/archive/2026-10-20-release-vector-metadata-names/verification.md) |
| 第三方 C 边界/编译诊断、nix 微秒类型及 cross 安装旧 lock | [诊断修复（后续构建生效）](changes/archive/2026-10-20-repair-third-party-build-diagnostics/verification.md) |
| OHOS 容器构建提交号 unknown 与 smoke 未校验身份 | [显式构建身份（后续构建生效）](changes/archive/2026-10-20-bind-release-build-identity/verification.md) |
| Behavior 切换丢失 retained Plan 与提交计数 | [Plan 持久快照](changes/archive/2026-10-19-preserve-plan-state-on-behavior-switch/verification.md) |
| 图片恢复 cursor 重连重复行及旧 spinner | [重连合并](changes/archive/2026-10-19-merge-image-recovery-reconnect-rows/verification.md) |
| child 图片恢复通知、真实历史回放及异常结束漏处理 | [child 图片恢复](changes/archive/2026-10-19-route-child-image-recovery/verification.md) |
| Windows storage CI 未覆盖构建配置路径 | [CI 触发范围](changes/archive/2026-10-19-fix-windows-storage-build-input-triggers/verification.md) |
| 同步 compaction 清空当前 TODO | [保留计划投影](changes/archive/2026-10-19-preserve-planning-through-compaction/verification.md) |
| 工具运行时失败继续采样、原始错误被后续 ACK/Step 错误遮蔽 | [工具失败停止边界](changes/archive/2026-10-19-stop-on-tool-runtime-failure/verification.md) |
| child handoff 重复复制已有历史、重复校验已验证的冷 Timeline | [历史所有权优化](changes/archive/2026-10-19-reduce-child-handoff-history-copy/verification.md) |

挂起优化仅消除已经确认的重复工作。单次本机合成历史测量不代表端到端挂起耗时，也不能解释所有真实长会话等待；仍保留 live 历史验证、最新事实重读、writer/child 关系检查和既有期限。未来只有真实多节点负载证据才能支持进一步优化，不预建缓存或并行退役框架。

原审计证据见 [v2.4.0 后续审计](changes/archive/2026-10-19-audit-post-v2-4-0-boundaries/audit.md)。

本轮后续维护不修改已经发布的 v2.4.1 tag 或产物；历史 release 验证记录保持原样。
