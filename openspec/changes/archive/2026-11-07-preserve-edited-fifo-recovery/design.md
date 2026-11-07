# Design

## 当前事实

`actor/prompt_queue.rs::replace_queued_prompt_with_admitted_input`保持live InputItem.prompt_id和queue_meta.id，仅替换input_ids、blocks、meta；编辑payload的prompt_id为独立的 `original:edit:N`。`AdmissionResolved.supersedes`将原input终止为RouteSuperseded。`Timeline::pending_allowed_inputs`目前从物理Submitted次序挑出未终止输入，于是A′的位置落到B之后。`restore_pending_human_inputs`依查询次序push_back，并把payload.prompt_id写回InputItem.prompt_id，造成队列行/运行身份分叉。

既有F3要求接纳重试身份与不可变payload一一对应。不能通过把编辑payload的prompt_id改回原行来修恢复，否则新内容会与原请求身份冲突。`StoredQueueEntry`已经保存原id、版本、owner/editor、正文，不需另加持久身份字段。

## 所有者和契约

Timeline/Input fold拥有supersedes和FIFO有效性，既有pending_allowed_inputs查询应输出替换后的canonical顺序；Shell不另扫raw日志或在恢复时排序猜祖先。用既有supersedes继承被替换输入的最早有效槽位，支持连续替换及同一live行所含多个input的替换；没有替换的输入保留既有顺序，消费/Block/Dismiss/Reserved过滤保持。可以在既有private fold保存派生位置，不能新建第二份持久队列/事件或公共排序registry。局部表示由实现者按现有Timeline prepare/accept选择。

Shell恢复仍从immutable payload读取最新正文、图片和来源。FIFO item有StoredQueueEntry时，runtime prompt_id用该行id；payload.prompt_id继续作为这个编辑提交的接纳身份，由pending.input_id关联，不用它替代行id。无StoredQueueEntry的既有普通输入沿payload.prompt_id默认行构造。F1保留Turn的Steer恢复分支不改；Goal/FIFO仲裁和用户手动重排现有语义不扩大。

两侧没有新增wire/序列化字段。ChatState先交付查询顺序，Shell再交付恢复身份映射；最终验收用真实编辑入口和持久日志切点证明组合正确。

## 反例与验收

最小旧红：admit A、B后durableedit A，清空runtime并由canonical query恢复，旧实现得到B、A′且A′运行promptid不是A。扩展A/B/C、A两次/B一次，按每次durableACK的切点保存、销毁并重建owner，断言顺序、最新正文/版本、行和运行身份、旧input终态。包含同内容不同编辑身份、ACK未知后重试的单次Hook/Submitted、拒绝保存保留原行/hold，及带图片行保真；复用现有F1/F3和queueedit回归，不为只测getter增重复fixture。

若实际cold恢复入口暴露额外缺口，先定位当前契约及owner，不顺带引入全局队列持久化框架。显式reorder目前只改内存；本轮不宣称重启保留它，也不通过此修复改变该独立政策。
