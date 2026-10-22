> 2026-10-21：用户要求继续第三项实施与归档；按原完全合并范围继续。下文精简账本为未采用的备选分析，非当前执行方案。

# 实施前对抗审查：是否必须删除整个 updates

状态：C1、C2 已独立完成并归档。本 change 已补 release 恢复和正常提交基线，尚未改持久格式。以下是需要确认的范围修订建议，不代表原 proposal 已修改或本 change 已完成。

## 新的判断

建议保留一个精简的展示账本，删除 response projection 的正文、digest、写入和修复路径；不把所有展示 DTO 和写入通道一起搬入 ChatState。原方案把“消除第二份 response”和“删除整个展示账本”绑定在一起，后者并不是前者的必要条件。

这不是因为 Timeline 无法容纳这些事实，而是两条路径的主要收益相近，完全合并的额外代价明显更大。已授权的原方案明确要求删除 updates，所以不能不经确认就改成保留它。

## 最小替代结构

| 内容 | 唯一持久来源 |
| --- | --- |
| 模型 response 正文、准入身份、quarantine、执行事实 | Timeline，保持原 admission durable ACK。 |
| 第一个 candidate 的顺序位置 | 展示账本中的 payload-free typed anchor，先于 admission 持久确认。 |
| 确实无法从领域事实恢复的展示数据 | 展示账本中的现有 typed 唯一内容，保持原顺序、来源、脱敏、预算和确认边界。 |
| response 的历史输出 | 内存引用视图：anchor 的 request/attempt 关联 Timeline 中的已准入 response；没有 projection 磁盘记录。 |
| 正文完整性 | 新 Timeline 编码的原字节 checksum，替代第二份 response 比较；仍完整扫描、校验所有必要事实。 |

anchor 已落盘而 admission 不存在时不展示候选正文；admission 成功后不再等待第二次 projection 提交。有展示正文却没有流式 candidate 的 response，须在准入边界先确认一个 anchor，或在同一 admission 中保存足以证明展示位置的 frontier；不能拿两个文件各自的 seq 猜跨文件顺序。已有但无法证明关联的 anchor、身份冲突或损坏正文必须拒绝。fork/import/export 仍需迁移必要引用与 schema，不能以保留账本为由跳过这些读者。

resident/cursor 必须同时证明 Timeline 准入截点和展示覆盖；新 admission 若要回填已经越过的 anchor，不能仅靠 updates offset 做 append delta，覆盖无法证明时仍完整替换。这里保留跨账本关联的成本，但它只关联身份、位置和必要展示来源，不再重读并比较整份 response。不能把它称为单文件事务。它也不提供提前 ready、历史分页、执行检查点、压缩或后台全量恢复。

## 完全合并的额外代价，来自实际调用关系

- [来源盘点](source-inventory.md) 已确认 Workflow/Goal/Control 的 expectation 只有摘要，没有时点正文；用户 display echo 不等于模型输入；SubagentProgress 虽有 transient 注释，实际会持久化。直接删除会丢内容。
- `session/actor/timeline_persistence.rs` 中的 ChatState durable sink 等待 SessionPersistence 的 ACK。`tools/notification_bridge.rs` 的提交入口只有 PersistenceHandle，没有 ChatStateHandle。不能在 persistence 的写入处理中反调 ChatState，形成等待环。
- 完全合并可以通过逐个 producer 接回 owner 消除等待环，但它改变 anchor、工具桥、Goal/Workflow 辅助发布、直接 bash/steer/host echo 的入队与确认顺序；这超出了原先“只替换数据来源”的估计。
- finite typed 展示事实是可行的，但需要处理 Shell wire DTO 与领域 crate 的依赖边界。例如 ControlIntent 的 inherent 方法使用 ACP Error，不能只移动 struct 后仍在 Shell 实现其方法；不能为了省代码把整个 ACP/Grow envelope 塞进领域事件。
- 部分展示消息目前合并或缓冲后同步。全部改为逐条领域 durable fact 可能增加正常写入成本；已有 expectation/preview durable 消息则可能原位替换，不应笼统声称所有展示都会增加一次 fsync。

完全合并的独有好处是统一截点、身份与来源校验，减少长期双文件管理。保留精简账本的代价是仍有两套物理顺序和关联校验。本建议不是说全量合并错误，而是认为当前以恢复性能为目标，尚无证据证明必须支付全部合并成本。

## 两条路径的收益边界

- 两者都能消除一份 64 MiB response 正文；现代 2048 轮样本的两个日志共约 148.17 MB，粗略节省上限约 45%，会被新增 checksum/metadata 回补。真实完整目录含其他文件，不能按此承诺。
- 两者都可删除 canonical digest 构造、projection 正文解码/比较与 projection durable commit。精简账本仍需读取小型展示事实并按身份关联，完全合并也必须生成相同历史，二者均不是零成本。
- 正常提交基线：三个独立进程，32 KiB response 的进程内 median 分别为 41.215、31.235、41.405 ms。测量窗口为 anchor + admission + projection，不含完整 turn。每次对应两个 updates 和一个 Timeline durable append，共六个 ledger file/directory 同步探针点；不是全进程 fsync syscall 总数。
- 两者都可把该窗口的三次 ledger 提交减少为 anchor + admission 两次。删除约一次同步等待的理想贡献约十余毫秒；不能承诺整个 turn 加速三分之一，新增校验与其他持久化仍有成本。
- 最新恢复基线的 2048 轮 RPC 为 754–1085 ms，主机波动明显。不得拿最高的那次与优化后最低值对比。实施后应使用保留的 before executable 做 AB/BA 配对测量。

## 确认后的执行边界

若选择精简账本，在本 change 内修订 proposal、design、delta specs 和 tasks，再实施；C1/C2 的归档不受影响。若坚持完全合并，继续原范围，但需接受 typed 展示事实和 producer owner 通道的整体迁移，不能用 opaque 数据袋或删除历史覆盖测试缩短实现。
