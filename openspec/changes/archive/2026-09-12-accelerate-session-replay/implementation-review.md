## 审阅结论

本轮仅处理恢复链路中已测得的重复工作。保留 ScrollbackState 作为条目/布局所有者，复用 EntryRenderer 的 estimate_height、现有 measured 位及 viewport settle，不增加新的缓存、后台 worker、队列或运行时依赖。恢复期间的输入与完整加载屏障没有提前放行。

1. 已撤回的 Shell 候选：ACU 的原判断是 contains(prefix) && 首个 update 值 starts_with(prefix)，后者已隐含前者。删除全文 contains 不改变结果；嵌套 _meta、tool rawInput/rawOutput 和转义正文的保护仍由原位置确认及现有测试验证。
2. 已撤回的 Shell 候选：resident actor 的 stale-task early return 从扫描后移到扫描前。函数同步、扫描不修改权威状态，resident 判定期间无 await；cold load 仍重新读取 delta drain 后的 pinned ledger，不复用过时快照。
3. batch append 保留已有精确高度，新条目仅估计；dirty-height 处理同样推迟 off-screen 精确渲染。可见区、inline edit 与最终收尾继续完成精确布局。隐藏 thinking 的非相邻间距无法用现有 pairwise append 修复，回退原完整重建。
4. batch 中已扩展缓存必须先计入总高度，再应用 dirty delta，否则历史可能不能准确跟随底部，已增加专门回归。cache 缺失时保持原 O(1) 失效，不在每次插入收集全部历史 ID。
5. 正常非 batch 的渲染、turn rebuild、分组开关和显式失效路径保持。最外层 end_batch 仍重建 turn 与布局，嵌套 batch 不提前收敛。

两次只读子 agent 核对用于梳理调用边界和缓存风险；主 agent 复核并修正 benchmark 的 Action dispatch、32 条通知边界、mutable access、单调 eventId、输入穿插和完整性断言。未把微基准或 reducer 的单次输入处理时间当作终端 p95。

首轮单纯增量 append 的收益较小；加入 batch dirty-height 的可见区测量后，256 turns 布局阶段中位数由 292.94 ms 降至 130.23 ms。最终布局的小幅成本增加已计入总时间，未删除收尾测量。最终验证与磁盘记录见 verification.md。

Shell 全链路实验没有证实收益，最终恢复到本轮开始的源码；上述第 1、2 点只记录候选审阅，不属于最终交付。最终实现范围为 Pager 和测试/文档。
