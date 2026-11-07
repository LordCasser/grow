# Runtime tree evidence belongs to the validated owner

## 代码依据

ChatState actor/mod.rs GetTimelineEvents执行events[start..].to_vec，CanonicalChildFacts::read通过Timeline::from_events重放；Timeline::accept_replayed逐事件validate header/content、lifecycle、surface apply。Mvp runtime_snapshot_before在suspend_root_tree_locked的初始、freeze后及child退休后调用。live ChatState已在同一owner接纳/加载时校验其ledger，再从它复制整份历史重放不提供新的外部数据完整性保证；cold load_light也已返回validated Timeline。必须保真正未知来源的validate，而非为了性能将所有read改scan。

## 第一增量契约：ChatState provider

新增单一查询：

```rust
pub async fn runtime_tree_snapshot(
    &self,
) -> Result<(crate::session_runtime::RuntimeLifecycle, Vec<crate::TimelineEvent>), String>
```

同一个ChatState command内，从当前已校验Timeline复制RuntimeLifecycle，以及按原seq/顺序保留的全部Subagent::Spawned与Subagent::Ended事件。其它Subagent variants和其它kind不复制；保留原事件header、spawn recipe/prompt及terminal/result_ref身份，不redact字段或重造事实。来源已由canonical owner验证，此结果专用于Shell树协调；接口文档明确是完整的Spawned/Ended投影而不是完整Timeline，不能把它交Timeline::from_events当独立ledger。通信失败是Err，不是空facts或default runtime。扫描事件可线性，但复制量不得随无关history正文增长；无需缓存或新持久状态。

只实现ChatState command/handle/owner与单元测试，不改Shell。使用已有canonical fixtures验证runtime+seq/身份与full Timeline一致、无child空投影、大正文无关事件不进入返回、错误owner失败。必要源码surface为chat-state，API不依赖Shell。完成后父审provider，再交consumer；不把provider通过等同退出修复通过。

## 第二增量契约：Shell consumer

`runtime_snapshot`直接返回`(RuntimeLifecycle, CanonicalChildFacts)`。live handle调用runtime_tree_snapshot并用CanonicalChildFacts::scan核child identity/重复规则；cold/closed handle仍load_light得到已验证timeline，调用from_validated_timeline，不能再次clone→from_events。三个挂起snapshot使用同一contract；其它restoration/未知输入上的CanonicalChildFacts::read保留原完整验证，不全局替换。latest snapshot仍在child writer/finalization确认之后取得，不能缓存初次结果跨这个frontier。

单项Shell consumer包含挂起入口回归与性能判别。先用原consumer构造真实已校验长history session，经actual Mvp/ACP挂起调用记录失败阶段、elapsed与数据量；原红须是五秒未确认而非fixture未启动/字段缺失。provider落地后可保持原consumer作为baseline，切换consumer再跑同产物/等价数据新绿，记录固定工具链、执行配置和最终source。正常空会话、活动/已完成child、checkpoint后race、ACK未知及cold source失败保持。若代表性长history尚不复现五秒，只报告有界开销改善及继续定位；不得强行改预算或用skip屏障骗过测试。

## 必须保持

一个exit_id/owner合并、五秒绝对deadline与显式retry/force，leader按driver scope，冻结新准入，树身份/父子完整性，最后child fact再读，CommandsSettled/Checkpoint/TreeSuspended durable ACK，actor+persistence实际退役。失败仍unconfirmed且原worker保其责任；不删除/截断历史、不伪结束Turn/Goal、不把闭cmd入口当writer退役。

## 第三增量：启动图片清理复用已验证事实

真实约 291MiB 合法复杂历史 B 在树查询 41/55ms 后仍于 5002ms 返回 unconfirmed，最后阶段为 draining_services。父已核 teardown 对 notification reconciliation worker 必须 exact join；该 worker 并发运行三类清理，input_admission::reconcile_user_image_assets 的 blocking 调用无条件从磁盘读整份 Timeline 并 from_events，再进入 sweep 才检查 assets 是否存在。退出取消不能中断该重放。现场原 root 只有旧 images/，没有 assets/；B 同样没有 assets。此路径没有任何资产可删时仍付全重放成本，图片资产存在时也不应重新验证 live owner 已验证的事实。

冻结最小 API：ChatStateHandle::committed_user_image_paths().await -> Result<std::collections::BTreeSet<String>, String>。单 mailbox query 从全部已验证历史事件中的 Messages.items、Input::Consumed.item、Notification::Consumed.input 提取 User 文本内 image_files_envelope_paths，保所有历史已接纳引用，包括后来 compaction/rewind 不在 Surface 中的引用；其它 item/正文不复制，不要求新增 index/cache 或文件路径 grammar。通信失败 Err，不能返回空 set。

Shell 图片清理仍是 asset batch grammar/contained directory 删除的 owner。保现 input_artifact_gate 覆盖 live authority 查询到 blocking sweep，持有实际 session directory/writer epoch。把提取路径映射到原 image_batch_name 规则后，只把 retained batch set 交给现受控 sweep。startup live 清理不得读磁盘重放；未知/冷工具直接调用仍保原完整校验入口（若仍有真实 caller），不得把任意 events 当已验证。取消在查询前/后及每个待删 batch 检查；取消只停止最佳努力维护，不撤销已接纳写入，不 detach 已运行 deletion，也不放宽 exact join。查询失败不删除任何资产。允许 assets 不存在的普通 no-op，但不能只用该特例绕过 assets 存在时的全重放。

两个 owner 分别交付：ChatState query/测试；Shell 图片 worker及现 sweep 的窄适配、错误/取消测试和真实 B 复验。必须保持有效/历史 committed 批、不可识别目录、坏历史/owner 失败时证据不丢；证明 no-assets 与真实 retained+orphan assets 两种情况。先用现 B 失败日志及精确 service/图片阶段观测确认阻塞，再修；同 B 原 5s、durable checkpoint、writer retirement/prefix/cold 断言全部保留。

## 后续证据与Review Plan

Tier3。父核provider实际实现、Shell known-validated来源、调用方无投影当完整ledger误用；独立review关注cold validation/child丢失/晚到终态/失败owner。真实长history的默认CLI双Ctrl+D与恢复构成最终里程碑，必须绑定本次binary，确认没有force与实际退出0及原canonical可恢复。第一增量后若Session suspend_node的全量conversation/events确实仍占主要预算，再按其实际需要冻结窄查询，不预建通用过滤API。runtime_source_hash只需有效project rules却复制整会话也须先测，不与其他恢复语义一起重写。

父统一主spec/docs/backlog/archive；实施者只写各自模块和本change分项验证记录。默认CLI构建由父协调，Cargo窗口串行，性能结论要对比基线。
