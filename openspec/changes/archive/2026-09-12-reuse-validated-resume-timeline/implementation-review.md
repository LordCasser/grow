# 首批实施复核

已按批准范围完成 A1–A4 的实现和本轮验证。现有未提交改动保持，代码留在工作区；本轮没有提交、合并或发布。全局依赖与其他架构债务保持独立，未混入本次。

## 已落地

- **A1**：storage 已验证的 Timeline 直接移交 bootstrap，去掉重复 fold 和整份 Workflow 恢复副本，保留发布前校验与修复顺序。[验证](verification.md)
- **A2**：Grow-only 扫描跳过 ACP payload 的 typed decode，补齐有效 fixture、读取阶段计时及正常按键/队列回归。分批 drain 经实测变慢，已撤回。[测量与范围](../2026-09-12-optimize-session-history-replay/verification.md)
- **A3**：已知用量事实损坏或同身份冲突会在 actor 发布前拒绝；合法重复保持幂等，未知诊断 Observation 保留。[验证](../2026-09-12-validate-restored-usage-settlements/verification.md)
- **A4**：Context 成功/失败结果在所有状态写入前核对 session、binding epoch 和 modal nonce，覆盖迟到、关闭重开和重新绑定。[验证](../2026-09-12-fence-context-info-results/verification.md)

## 性能结论

128 turns 样本的 actor 阶段中位数从 115.2 降至 93.1 ms，约减少 19%；完整加载从 503.8 变为 493.7 ms，仅约 2% 变化。512 turns 各阶段只有一次探索，最终总耗时没有改善。不能据此认定用户反馈的完整加载慢、操作迟滞已经解决。

正常字符输入与 Enter 排队/释放的 reducer 回归、实际 --continue PTY 均通过；这些没有测出真实终端输入回显 p95。后续应采集真实长会话的 Pager handler、布局、终端写入和完整响应 trace，覆盖工具密集、多子会话及 resident/cursor。目标和边界保留在 [backlog](../../../backlog.md)。

## 验证

- ChatState：483 passed、1 ignored。
- Pager：7198 passed、10 ignored，含新 Context 归属和加载期间按键测试。
- Shell：全量 3802 passed、1 failed、3 ignored；唯一失败是 fixture 把同 child 重复结算当作四个 child 计费。修正身份并增加重复不累加断言，相关 16 项定向复验全部通过。
- CLI 构建通过；PTY --continue 的历史恰好一次与后续提交通过。原测试缺少 mock 配置，补齐后才运行到实际恢复路径。
- 12/128 turns 的独立 ACP 历史顺序校验通过，比较 load response 前全部 user/assistant 通知与 typed reference，检查无丢失、重复和错序；perf/RSS harness 均编译通过。没有运行大型 RSS/dhat 场景或全部 ignored PTY。

原始结果摘录见 [validation-results.txt](validation-results.txt)，相关源码状态见 [code-snapshot.json](code-snapshot.json)。OpenSpec 最终结果见各 change 的验证记录。

## 磁盘

构建串行复用同一 target，未创建 worktree 或复制真实会话。合成 fixture 生成前设 256 MiB 预算，结束自动清理。构建后按创建时间清单回收本轮新建的 244 个增量缓存目录，保留 36 个原有目录以及 CLI、依赖产物和测试二进制；可用磁盘由约 43.2 恢复到 61.0 GiB。清理清单见 [disk-cleanup.json](disk-cleanup.json)。

四个 change 已独立归档，行为 delta 已合入主规范。最终严格校验 17/17、归档校验 327/327 通过，原有三个 active changes 未改动。临时测试日志与格式化副本已清理，关键结果摘录与性能原始输出保留在这些归档中。
