## 独立对抗性审查
reviewer：/root/rules_recovery_review，仅静态独立审查，未运行 Cargo。依据任务起点 snapshot 与 scoped Atlas + 调用方核对。

初审发现 P1：新 child 也以 Existing 传入，取消恢复补注入将导致普通 child/normalized fork 缺规则。修正为 Seeded 区分新任务 seed，在首次规则持久确认后继续；restore 仍 Existing。新增真实 Spawn/provider 请求三路径测试。

复审确认原 P1 修复：durable ACK、mirror/已有 typed skip、prefix 长度以及 classifier 有效规则处理正确，未发现新阻断问题。Agent rebuild 保留 Surface；常规 compaction/rewind 保留前缀，未发现新增可达不同步路径。
