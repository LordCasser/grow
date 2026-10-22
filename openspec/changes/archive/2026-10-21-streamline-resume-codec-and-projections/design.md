# Design

## 最小实现

1. 版本探测只读取窄 envelope；Timeline kind 的 typed 解码不再经过整棵 `Value`。可以使用现有 serde/RawValue；“正文只做一次领域解码”不等于 JSON 字节只扫描一次，不为追求零扫描自研 parser。
2. 在单次规划作用域借用已校验 response；branch provenance 只计算一次，现有共享正文只增引用。避免给 Timeline 的所有公开查询加持久增量索引，也不把借用跨 actor/await 扩散。
3. 从 projection 生成 typed notification 后，复用同一个转发入口完成 metadata、工具合并、provenance 与 completion；raw 普通更新仍按原契约解码。去掉再编码后递归调用 raw parser 的往返。

## 对抗性核对

| 可能退化 | 约束与验证 |
| --- | --- |
| 窄解码意外接受未知字段、重复字段或错误 schema | 保留当前版本错误、完整行/大小限制、领域拒绝边界；原 Value 会覆盖 payload 重复键，直接 typed 解码明确拒绝这类歧义，delta 和回归覆盖该差异。支持原始 JSON 与 Value 调用方，不引入另一套 JSON parser 或宽松 fallback。 |
| 借用跨 branch/模型路线变更后失效 | 仅同一固定 Timeline revision 使用；路线补写、rewind/compaction 后重新派生，不建立跨 load cache。 |
| typed 快路漏掉 metadata 或改变工具顺序 | typed/raw 入口共享末端转发，比较相同通知语义、source、工具合并与顺序；不绕过 completion 屏障。 |
| 过度宣称去掉复制 | 大文本已用 Arc；重点消除中间 Value、序列化和临时容器，分别量化分配与耗时。 |
| 提早 yield 改变 resident/live gate | 本项不改变调度、不拆 load barrier；loading 普通输入仍忽略，Ctrl+D/resize 保留。 |

## 理论收益与代价

同轮 R6 debug 中 Timeline read/decode 931 ms、forward 1576 ms 是受影响的父阶段，均包含必要工作，不能把合计 2507 ms 当作可删除时间。保持 O(历史字节 + 事件数)，降低解码/分配常数和临时峰值；不减少磁盘体积，不改变 durable commit 延迟目标。

代价是窄 wire 与领域类型之间需维护映射，以及少量借用 API；不新增持久实体或运行 owner。先建立固定工具链 release 基线，再分别报告各阶段和端到端收益。与后续删除 projection 的收益存在重叠，不相加。共同基准见 [分析入口](../../profile-release-resume-first-history/timeline-reorganization.md)。

## 核对入口

- `chat-state/src/timeline.rs::TimelineEventWire`、`admitted_responses`。
- `shell/src/session/storage/jsonl/mod.rs::read_versioned_jsonl_from_directory`。
- `shell/src/session/response_projection.rs::project_admitted_response`、`plan_response_projections`。
- `shell/src/agent/mvp_agent/mod.rs::forward_raw_replay_line`。

路径均位于 `crates/codegen/`；当前源码为准，Atlas 本轮局部查询未返回已有目标符号，未以其空结果推断无调用方。
