复用已有 JournalData/HistorySnapshot 的 checksum、序列和来源绑定校验，Timeline::from_events 验证 canonical 因果，HistorySnapshot::validate 与 validate_owner 保持结构/identity 约束。删除不会构建模型或展示 Surface，因而不需要读取 sideband、prompt blob、notice payload 或 sampling body 来证明其可恢复。禁止通过调大全局常量或伪造完整 ValidatedTimeline 绕过预算。

OpenedSession 抽取已有 journal error mapping 供 timeline_events 和删除读取复用。delete_opened_session 在原 writer lease 下读一次 canonical Timeline，传入引用检查；其余 session 的 canonical facts 同样校验。显式请求的工作量随真实 ledger/会话规模增长，保留 per-record 64 MiB 边界；自动维护/只读预算不变。

Tier 3：有效 sampling chunk 证据在维护预算外仍能显式删除，>16 MiB 合法 journal 可删除，损坏 journal 仍拒绝，真实跨 session 与 unfinished runtime guard 不放宽。所有 fixture 使用隔离临时根。
