# Design

## Owner and current evidence

`IndexManagerHandle::send_event/send_events`把有序事件交给codebase-graph actor，查询同一通道上的GetSnapshot形成已处理屏障。`CoalescedEvents::insert`当前对(Created|Modified,Removed)执行Entry::remove，`apply_coalesced`因此看不到删除。真正的`ScopeGraphIndex::remove_file`同时清理该文件的definitions/references/file metadata，并支持删除未知路径。Shell的`CodebaseIndex::on_change`及Git刷新只提供事件，不持有索引内容；修复归归约owner。

归约器没有批次前索引状态。Created并不证明路径从未被索引，例如替换或rename到已有目的地，所以Created→Removed也必须保留最终删除。不新增初始状态cache或查询索引来猜，直接让幂等remove_file兑现事实。Removed→Created/Modified继续代表现有替换语义；rename仍拆成source Removed与target Created，target随后Removed必须独立清理。该结构足以修复已证缺陷，不修改watcher或全量重建策略。

## Coder Task Spec

一个codebase-graph功能：同批次以删除结束的路径不再残留符号与metadata。写范围仅现`src/index_manager.rs`的CoalescedEvents及同crate相关测试，必要私有测试子文件可按现约定使用，不增加生产公共接口、持久状态、依赖或新的模块层。同步本次改变的coalescer注释与旧错误预期。父拥有OpenSpec、开发说明、backlog和归档。

保持现非删除合并规则、同批次路径顺序语义、rename拆分、hidden/unsupported排除、COW快照、cache与线程寿命。不要把本项扩大为文件读取竞态、cache发布或语言query修复。

## Verification and review

Tier3数据一致性验证：先在未改生产上增加真实公共Handle回归，初始索引至少有一个真实Rust文件且defs、refs、metadata确实存在；磁盘删除后以同一个send_events batch提交Modified→Removed，GetSnapshot确认三类事实均消失，当前生产应明确失败而非compile error/空filter。再修最小归约并运行同项green及相关coalescer/index-manager回归。

同一行为的边界覆盖Created→Modified→Removed（包括已有索引）、Removed→Created→Removed、rename目的地原已索引且最终删除、Created→Removed未知路径幂等、Removed→Created确实重新索引和独立路径不受影响。断言外部最终索引，不只看map；保留rename、非删除与旧snapshot不变对照。算法细节由coder选择，不为每种序列新建框架。

Rust/Cargo/rustdoc1.93.1、offline/locked、MINSTACK16777216、JOBS2、默认target，隔离/tmp Grow/TMP/Git环境；Cargo由父串行派窗口。源码冻结后Luna独立review精确增量/真实消费者，父亲读red/green、snapshot断言与最终hash。该公共Handle→actor→ScopeGraphIndex链即本项集成验收，不重跑全部Shell/CLI或真实OS watcher；平台通知送达和未接纳事件不在本项承诺内。
