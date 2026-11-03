## 所有权
沿用ChatStateActor串行命令和现有durable helper，不建立缓存/dirty bit/另一套repair检测器。Result<Option<Vec<ConversationItem>>>只表示是否已有可复用副本：未改Some(candidate)，changed commit后None，错误仍Result。builder None才clone已提交Surface；push_user_message_durably丢弃Option，不增加changed后的多余clone。

MemoryContext的Messages/Append应用只追加同一个typed item，无额外context激活。owned items在成功ACK后追加同item；already_present不追加。不能提前移走canonical Surface，也不能在未确认repair/memory时返回请求。

## 验证
先读[审查计划](../2026-11-03-close-nondeferred-backlog-and-build-release/c4-review-plan.md)中精确harness与已有false positive测试的边界。主spec session-timeline要求修复原始facts保留、durable后发布、cold一致；本次不改这些要求。健康普通text、tool-heavy、image Arc负载128/512/2048/8192比较旧双clone和新单clone，setup/相等检查在计时外，warm2+7samples保存源码/原值；Arc正文不是深拷贝，不声称整request/provider提速。真实请求测试覆盖repair ACK→memory ACK→返回、exact event retry、两处取消，以及旧Goal仅request shadow和重复reminder不多写。全ChatState验证后独立review。

健康候选可复用前提已核对：normalize_model_skill_results仅needs_reorder分支改items且同时changed+=1；隔离/重复/移除/补结果也仅在各report计数非零时改变结构。report.changed=false并非只检查dangling，不会把未报告修复的candidate带入请求。C2更改这几处算法后需保持该前提。
