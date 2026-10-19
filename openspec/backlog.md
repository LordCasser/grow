# 未解决边界

2026-10-19 按用户授权核对并修复 v2.4.0 后续审计四项，以及 backlog 中三项可验证问题。七项分别立项、补齐回归、对抗性审查并归档；MCP 不在本轮修改范围。以下保留此前明确暂缓或缺少真实负载证据的方向，不扩大为新功能。

- **Remote Control / Agent Host 跨设备使用（暂缓、未排期）**：用户认可从其他设备连接和操作已有 Grow 会话的方向，但当前不实施、不设日期，不纳入 MCP 多轮或保留计划提案。本次不展开远端控制架构，也不因此引入统一 Dock、Memory 或其他大型功能。未来明确启动时另建 change，重新界定身份、权限、执行 owner、断线和恢复边界；本条不构成实现授权。决策见 [2026-10-11 上游主题评审](changes/archive/2026-10-18-record-upstream-review-2bdd1d6/review.md)。

- **Provider 缓存策略与 native epoch 的关系**：[理论 review](changes/archive/2026-09-28-evaluate-provider-cache-affinity/results.md) 只能确认当前 key、breakpoint 与 retention 的本地机制及各官方契约，不能证明统一调整会改善多 provider 的成本与延迟。若要优化，先确认具体 provider/model/deployment、字段透传与负载，再按该 route 的真实证据另开最小行为 change；不预建通用 cache policy registry。
- **Recap 与主请求缓存的关系**：recap 使用独立 key 且没有主请求 tools，已修正把本地 wire 形状表述成远端复用的注释和测试名。真实共享前缀与收益仍未验证；保留 Sideband tool-free 契约，不能为缓存假设增加工具定义。

- **持久 Bash 的快照局部名称遮蔽用户导出变量**：`dump_bash_state` 在 `export -p` 前声明 `env_vars`，会遮蔽同名用户全局变量；这是既有捕获边界，独立于本轮修复的 allexport 大载荷启动失败。本轮保留范围并记录，未来另建最小 change 处理捕获命名与用户值保留，不能把修复说明理解为任意同名变量都可无损恢复。证据见 [Bash 导出隔离验证](changes/archive/2026-10-19-isolate-shell-snapshot-export-state/verification.md)。

- **动态项目配置 watch 的生产回收尚未接入**：生产调用会向 ConfigFileWatcher 注册 cwd，当前 `unwatch_path` 仅由 API 测试使用；本轮只修复既有注销契约和迟到回调，未接入新的 session teardown 生命周期。长期 leader 多 cwd 的 watch 回收需另建 change，先确认各生产注册者的共享引用所有权，不能直接在任一 session 关闭时注销共享目录。证据见 [配置监听注销验证](changes/archive/2026-10-19-reject-orphaned-config-watch-events/verification.md)。

- **第三方交叉编译告警与 sqlite-vec 边界**：v2.4.1 首次候选的 GCC 构建暴露上游 sqlite-vec 的 pragma、符号比较及可能未初始化诊断，其中 `vec0_metadata_filter_text` 的 `size_t metadataInIdx = -1` 后检查 `< 0` 是无效哨兵判断；尚无 Grow 运行时触发复现。Grow 当前 vec0 schema 只有 chunk_id/embedding，查询为 MATCH/k，不使用 text metadata IN。供应商 C 与 v2.4.0 相同，既有打包 workaround 要求上游文件保持一致；后续另立最小 change 验证边界并选择上游修复或明确维护补丁，不能仅消音或声称所有诊断无害。另有 nix 0.26 的 musl `suseconds_t` 弃用提示，以及安装 cross 0.2.5 时其独立 lock 中 dunce 1.0.2 的撤回提示（Grow lock 为 1.0.5）；分别按依赖约束处理，不混入 MCP。证据见 [平台告警维护验证](changes/archive/2026-10-20-clear-platform-release-warnings/verification.md)。

- **精确测试 runner 的阶段可观测性与子进程期限**：`scripts/run_exact_cargo_test.py` 的 list/run 两次 `subprocess.run` 都先完整缓冲输出，且无子进程级 timeout；成功仅在整项结束后打印测试名称。Core job 的 90 分钟总期限仍能阻断发布，但运行中难以区分编译、枚举与测试等待，取消时的子孙进程收尾也需跨平台验证。后续独立 change 可先拆清各阶段耗时和进度，再制定编译/执行预算及 Windows/Unix 清理；本版不臆测长 PTY 步骤就是死锁，也不通过缩短期限或跳过用例掩盖问题。

- **OHOS 产物未嵌入 Git 短哈希**：2.4.1 OHOS 容器冒烟打印 `grow 2.4.1 (unknown)`；`crates/codegen/cli/build.rs` 在 `git rev-parse --short HEAD` 失败时使用该回退值，当前不保留失败原因。工作流仍按不可变完整 SHA 检出并为产物生成来源证明，因此版本及外部来源绑定可校验；后续独立维护需验证容器中的 Git 元数据可见性/调用失败原因，再补齐内嵌标识，不能把容器冒烟等同于真实设备验证。本次不为该展示元数据更换已完成门禁的发布候选。

## 本轮已解决

| 问题 | 归档与验证 |
| --- | --- |
| Behavior 切换丢失 retained Plan 与提交计数 | [Plan 持久快照](changes/archive/2026-10-19-preserve-plan-state-on-behavior-switch/verification.md) |
| 图片恢复 cursor 重连重复行及旧 spinner | [重连合并](changes/archive/2026-10-19-merge-image-recovery-reconnect-rows/verification.md) |
| child 图片恢复通知、真实历史回放及异常结束漏处理 | [child 图片恢复](changes/archive/2026-10-19-route-child-image-recovery/verification.md) |
| Windows storage CI 未覆盖构建配置路径 | [CI 触发范围](changes/archive/2026-10-19-fix-windows-storage-build-input-triggers/verification.md) |
| 同步 compaction 清空当前 TODO | [保留计划投影](changes/archive/2026-10-19-preserve-planning-through-compaction/verification.md) |
| 工具运行时失败继续采样、原始错误被后续 ACK/Step 错误遮蔽 | [工具失败停止边界](changes/archive/2026-10-19-stop-on-tool-runtime-failure/verification.md) |
| child handoff 重复复制已有历史、重复校验已验证的冷 Timeline | [历史所有权优化](changes/archive/2026-10-19-reduce-child-handoff-history-copy/verification.md) |

挂起优化仅消除已经确认的重复工作。单次本机合成历史测量不代表端到端挂起耗时，也不能解释所有真实长会话等待；仍保留 live 历史验证、最新事实重读、writer/child 关系检查和既有期限。未来只有真实多节点负载证据才能支持进一步优化，不预建缓存或并行退役框架。

原审计证据见 [v2.4.0 后续审计](changes/archive/2026-10-19-audit-post-v2-4-0-boundaries/audit.md)。
