## 基线与独立闭合

- [x] 1. 冻结并核对独立验证债务八项基线；标明各自 owner change、证据边界和明确暂缓项；保留原始快照，闭合后移除非延期项。
- [x] 2. 为 macOS watcher/MCP 失败建立独立行为 change；修复 owner/cancellation 边界并验证原 Pager、Shell 失败及相关全组门禁。
- [x] 3. 定位原树 cold-load/retirement 的等待 owner，修复 watcher owner 与初始 MCP grant 冻结竞态，保留出生权限事实与逐次授权；运行原 exact 正负例及 Shell 回归。
- [x] 4. 为子任务观察跨层债务建立独立验证 change；完成 SessionActor 终态/干预/receipt ACK、ChatState 停答/积压、结果 worker 饱和、panic/平台和注册丢失 vs 模型忘记 ID 的证据。
- [x] 5. 完成外部变异测试资源策略决定及受控验证；明确预算来源、临时副本、interrupt 和恢复边界，并由独立生产 change 处理 Grow terminal owner cleanup。
- [x] 6. 为剩余辅助指定模型入口完成跨 endpoint/backend/认证域验证或最小修复；Memory Flush 仅按既有证据关闭其自身范围。
- [x] 7. 完成 Responses 单工具/parallel PTY fixtures 的真实 Responses backend 验证。
- [x] 8. 完成临时目录 skipped/written 回执在 storage、flush、session-end 的修复和回归。
- [x] 9. 完成 task cancellation entry scope 的实现、拒绝/幂等对抗性测试和 SessionActor receipt 验证。
- [ ] 10. 为八项逐条补充实际命令、结果、日志/源码身份和残余平台边界；只在证据满足矩阵后关闭对应项。

## 最终发布门禁

- [x] 11. 在最终 candidate source 上运行发布相关 workspace 检查、原 18 crate 核心套件、格式与 OpenSpec 全量/归档校验；保留失败和 skipped 计数。
- [ ] 12. 验证 clean committed HEAD 与 annotated v2.6.0 tag 一致，执行 `bash scripts/validate-release.sh v2.6.0` 并记录官方 required gates、平台构建与资产来源结果。
- [ ] 13. 对最终闭合矩阵进行只读对抗性 review；发布阻塞未闭合时不得把候选标成可发布。
