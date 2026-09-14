## 调查与约束

先使用上一批保留的 debug 二进制采样，区分文件读取、解析、字符串扫描、通知应用和布局。Atlas 当前 scoped search 返回的 replay 签名与已修改源码不一致，符号位置和实际边界以当前文件、调用方及编译器为证据，不启动全库索引。

保持已验证 Timeline 所有权、pinned file、rewind/cursor、末尾未提交记录、控制与 usage boundary，以及 load response 屏障。上一批 128 行 drain 的吞吐退化已经有数据，本轮不重复采用该方案。

合成 session 不超过既有 256 MiB 预算，诊断输出单份不超过 32 MiB，不复制真实会话。Cargo 串行并复用同一 target，不切换全局 profile 或重建全库 release；记录本轮新建增量缓存，验证后有选择地回收。机器可用空间低于 20 GiB 时停止重型构建。

## 验证方向

在相同 fixture 上对照优化前后结果；新增测试应证明语义等价和加载期间输入保留，而不是只检查实现形式。真实终端 p95 未测得时明确说明。最终设计决策随采样证据补齐。

## 本轮实施决策

受限 Pager 基准使用真正的 ACP handler、TextBlock、每 32 条通知的 prepare_layout、穿插键盘输入和 SessionLoaded 收尾。128/256 turns 初测布局合计约 154/298 ms，handler 约 85/101 ms（第一次含冷初始化）。批量 push 在每个新增 entry 后丢弃已有布局，下一帧重新估计整个历史并重做可见区/上方预热；end_batch 仍会最终重建一次。

本轮复用现有增量 append 布局路径：batch 内新增 entry 只计算便宜的估计高度，标为尚未精确测量，继续由现有 viewport settle 完成精确布局。turn 索引仍在最外层 end_batch 收敛，分组、宽度变化、移除和显式失效仍走原重建规则。隐藏 thinking 涉及跨条目间距，批量增量扩展遇到该边界时保守回退重建。这样不新增缓存实体，也不把历史 Markdown 渲染搬进每条通知。用混合块、嵌套 batch、宽度变化和分组测试核对最终布局。

Shell 只去掉 ACU 判定中被后续 starts_with 隐含的全文 contains；仍按第一处 update key 做位置确认。另将已有 resident actor 保护提前到 stale-task 文件扫描之前；该函数同步且扫描无副作用，保护条件和返回结果不变。其余 rewind、subagent 和 stale-task 阶段暂不合并：后者在 flush/delta 屏障之后重新读文件，不能直接复用旧快照。微基准未发现 memmem 替换的收益，不添加新依赖。

第二次对照发现仅复用 append 缓存仍会在 dirty-height 路径精确渲染所有新收到的历史文本。本轮将同一 batch 的 dirty-height 更新也按估计值处理，显式 inline-edit 高度继续精确；只由 viewport settle 渲染实际可见条目。先按已扩展的缓存重新计入总高度，再叠加 dirty delta，保证跟随底部正确。完整 load 收尾仍重建并预热上方 3 页，相关成本计入 final_layout，不从统计中隐藏。

## 最终收敛

Shell 候选全链路中位数 128 turns 536.2 → 588.5 ms；512 turns 1377.4 → 1477.2 ms。未经修改的 actor/forward 等阶段也一起变慢，现有测量不足以将差异归因于这两个局部改动；不能据此宣称收益。谨慎起见，撤回本轮全部 Shell 生产修改，保留 Pager 有稳定阶段收益的实现。Shell 恢复源代码与本轮开始相同，旧优化和验证边界保持。详细对照保留用于后续调查，不伪装为最终代码的性能结论。
