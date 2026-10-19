# 未解决边界

已解决事项的设计、验证与归档记录见 [changes/archive/](changes/archive/)。用户于 2026-10-19 授权逐项结合代码核查、立项和修复；本轮确认结果与验证状态见 [backlog audit](changes/archive/2026-10-19-prepare-v2-4-0/backlog-audit.md)。确认缺陷已分别修复、验证和归档；下面只保留明确暂缓的产品方向，以及依赖真实负载证据的优化假设。用户再次确认本轮只修已确认缺陷，以下三项保持暂缓。原记录中的“后续/本次不改”描述的是登记时范围。

- **Remote Control / Agent Host 跨设备使用（暂缓、未排期）**：用户认可从其他设备连接和操作已有 Grow 会话的方向，但当前不实施、不设日期，不纳入 MCP 多轮或保留计划提案。本次不展开远端控制架构，也不因此引入统一 Dock、Memory 或其他大型功能。未来明确启动时另建 change，重新界定身份、权限、执行 owner、断线和恢复边界；本条不构成实现授权。决策见 [2026-10-11 上游主题评审](changes/archive/2026-10-18-record-upstream-review-2bdd1d6/review.md)。

- **Provider 缓存策略与 native epoch 的关系**：[理论 review](changes/archive/2026-09-28-evaluate-provider-cache-affinity/results.md) 只能确认当前 key、breakpoint 与 retention 的本地机制及各官方契约，不能证明统一调整会改善多 provider 的成本与延迟。若要优化，先确认具体 provider/model/deployment、字段透传与负载，再按该 route 的真实证据另开最小行为 change；不预建通用 cache policy registry。
- **Recap 与主请求缓存的关系**：recap 使用独立 key 且没有主请求 tools，已修正把本地 wire 形状表述成远端复用的注释和测试名。真实共享前缀与收益仍未验证；保留 Sideband tool-free 契约，不能为缓存假设增加工具定义。

## 待独立核对

- **同步 compaction 的 TODO 展示清理**：`pager/src/app/acp_handler/session_notification.rs` 在 `AutoCompactCompleted { async_compact: false }` 时直接清空前端 TODO，而当前局部计划由 Timeline 持有。需独立核对同步压缩后的计划再投影及恢复顺序，确认是否存在暂时或持续丢失任务入口；本次 Plan 顶部入口及 Agent 切换修复不扩展到压缩路径。
