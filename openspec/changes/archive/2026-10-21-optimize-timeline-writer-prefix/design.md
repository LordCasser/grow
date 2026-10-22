# Design

load_timeline_prefix 仍在既有 append 锁内，将读取限制为 complete_len，每条记录最多 MAX_JSONL_ENTRY_BYTES+1，要求完整换行并校验长度和 JSON。原始字节逐条进入 BLAKE3。成功读完整个上界后把拥有所有权的 events Vec 交给 Timeline::from_events；其私有 accept_replayed 验证 header/content/lifecycle，失败不暴露折叠对象。避免为每条事件复制累积生命周期。

保留 consumed 长度校验、metadata stamp、prefix hash/hasher、同长度篡改拒绝、tail seq/payload 检查、torn-tail 修复和 durable ACK。批量折叠不承诺一个同时违反多条约束的事件具有与 live accept 相同的首个错误文本；错误类别及拒绝边界保持。事件 payload 移动而非深复制，短时多一份 Vec 元素槽位。

验证沿用现代 admission/projection 长会话128/512/2048轮、每轮32KiB，512轮每16轮缺一投影，以及4KiB对照。记录修改前后完整 RPC、首条历史及阶段耗时，分别保存原始结果。前缀边界和 durable 测试覆盖 hash/长度/非法事件/替换/同步失败；最终用真实 CLI/PTY 核对历史推进、普通输入忽略、退出及恢复。
