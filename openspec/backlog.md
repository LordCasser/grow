# 未解决边界

已解决事项的设计、验证与归档记录见 [changes/archive/](changes/archive/)。以下事项尚不构成生产行为变更授权。

- **实时未知 stop reason 的成功分类**：`pager/src/app/agent_view/turn_completion.rs::finalize_durable_turn_terminal` 的 `pr_ok` 只排除 cancelled/error/rate_limit；例如 interrupted 或未来扩展原因可能走 Completed。此次 export/replay 保留原始 stop reason，不改实时执行 UI。后续须单独定义协议终态分类及旧 terminal 去重场景，不能把未知原因默认当成功。
- **离线展示缓存的完整重建**：export/replay 复用 response reconciliation，并恢复已验证 Hook、child lifecycle、parent/agent reply receipt 和 Behavior/Goal 的 Control 截点；但普通输入、turn terminal、Workflow 及控制中间过程在 updates 部分丢失时，还没有统一的离线重建/缺口检测。后续 change 应明确 Timeline 到展示 identity 的映射、缺失缓存的失败/降级语义，再抽取必要纯投影；不得以近邻时间猜测 turn 归属，也不得启动 actor 修复来源。细节见 `harden-transcript-interruption-snapshots` 与 `harden-offline-behavior-goal-projections` 审计。
- **实时 Workflow clear 的 revision 护栏**：`pager/src/app/agent_view/workflow_ingest.rs` 对 cleared 跳过旧 revision 检查，较旧的正 revision clear 可能停止较新 Run 的当前展示。离线消费者已校验该边界；实时消费者的无 revision clear 兼容语义、重连 hydration 与终态历史应另开最小 change 验证，不混入离线修复。

- **Plan 问题的部分自由输入丢失**：`QuestionViewState::build_partial_answers` 对 `Chat about this` / `Skip interview` 只发送 `Other` label，舍弃已经输入的自由文本；这与普通 Accepted 回答的 `annotations.notes` 路径不同。若需要在这两个 Plan 动作中保留用户输入，须单独定义部分答案的 wire 与模型结果语义，并验证重连及展示；不混入本次普通 Accepted 记录修复。
- **Provider 缓存策略与 native epoch 的关系**：[理论 review](changes/archive/2026-09-28-evaluate-provider-cache-affinity/results.md) 只能确认当前 key、breakpoint 与 retention 的本地机制及各官方契约，不能证明统一调整会改善多 provider 的成本与延迟。若要优化，先确认具体 provider/model/deployment、字段透传与负载，再按该 route 的真实证据另开最小行为 change；不预建通用 cache policy registry。
- **Recap 与主请求缓存的关系**：recap 使用独立 key 且没有主请求 tools，已修正把本地 wire 形状表述成远端复用的注释和测试名。真实共享前缀与收益仍未验证；保留 Sideband tool-free 契约，不能为缓存假设增加工具定义。
- **搜索缓存自愈 epoch 的 root 隔离**：`shell/src/session/storage/search_recovery.rs::CACHE_EPOCH` 是进程全局值，而搜索索引按 root 建立。并行测试中，一个临时 root 的自愈可使另一个 root 的 bootstrap 返回 `RunAgain`；现有 worker 能继续重试，但多 root 场景可能有不必要的重建。后续独立 change 应按索引身份限定 epoch/claim 失效范围，并验证不同 root 并发自愈、同 root 替换与完成标记的关系；本次发布只修正测试对合法 `RunAgain` 的一次性完成假设。

- **历史人工等待与进程离线边界**：`PendingInteraction/InteractionResolved` 不持久化，普通 Tool Started 早于权限 gate，冷恢复 terminal 的 duration 又包含进程离线时间。旧历史无法精确区分全部人工等待、执行与关闭时间；[Replay 交互方案](changes/interactive-readonly-replay/design.md) 本次仅用已有事实保守保护、明确估算恢复空档并允许显式前进。若需准确恢复所有等待，应另开 change 定义带 owner/operation 身份的 durable 等待/离线边界与恢复语义，不借 Replay 引入 heartbeat 或改写旧来源。
