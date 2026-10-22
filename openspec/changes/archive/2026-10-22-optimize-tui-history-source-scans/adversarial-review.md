# 对抗性审查

## 边界与代价

- 负筛选必须覆盖 `![` 的相对 Markdown 路径和 `/`、反斜杠的 Unix/drive/UNC 裸路径，保留先 Markdown 后裸路径的去重/alt 语义。代码、斜杠密集和误候选文本可能没有收益，独立报告而不外推纯文本成绩。
- 借用只用于已规范化 source 的读取，不能触发 renderer 的派生物化或跨后续可变借用。raw/pretty copy、late append、placeholder 和 wrap generation 沿用原路径。
- 图片仍在 message completion 读取和探测；仅复用同一 bytes 的维度结果。既有探测是 header/dimensions，不宣称完整像素解码或锁定外部图片后续内容；不能跨消息缓存文件状态。
- Mermaid 候选检查必须保留 ASCII 大小写和含实体的 fence；候选检查不能替代 CommonMark 解析，不改误候选的最终语义。无分配实现也必须测，避免逐字节低效搜索比原标准库小写更慢。
- 不通过提高帧率、扩大 batch、减弱同步/校验或提前 ready 取得成绩。CPU stage、frame 生产、writer flush、ACK 观察、PTY 可见各有不同边界，嵌套时间不能相加。

## 诊断审查与修正

Luna 只读审查发现 gateway 的 completion API 不暴露 enqueue accepted 状态，因此 `first_send` 严格是合并后的首次 `forward_attempt`，不是成功交付证明。新增字段明确边界，分析器必须核对同 session/cursor 的 first_applied 和成功 writer delivery/ACK；缺失链路不计成功。

审查也指出独立 extensions/agents 等覆盖层可能使普通正文首帧误报；补用既有 modal 判定及独立 viewer/交互 gate，过滤不可见根帧。普通根会话首帧不覆盖 fullscreen child/Dashboard 场景，这些不伪造数据；子会话仍按既有归属/展示测试验证。first_frame 代表首个应用历史后的普通根帧，可能因快速滚动已显示更晚消息，不是该 cursor 对应物理行的可见证明。writer_sequence 与现有成功 delivery、frame_ack 匹配以确认输出。

每次 replay window 重置两个 opt-in 字段；失败/被取代的 reload 清除，快速成功后仍允许捕获首次实际输出。字段不参与 owner、cursor、高水位、ACK 或 ready 判断。审查初稿提到的 `history_cursor.or(event_id)` 是前项既有逻辑，本项只增加诊断调用，不改变选择规则。

## 最终源码与数据复核

独立 Luna 只读审查确认负筛选是既有正则的必要条件，保留相对 Markdown、Unix/drive/UNC 候选、Markdown-first 的去重/alt 优先级；借用的是与旧 clone 相同的 normalized source，结束后才进行可变派生解析；同 bytes 复用维度探测不改变文件观察时刻；ASCII Mermaid 检查保留实体保守路径。没有发现扩大当前架构范围或改变用户行为的必要性。

独立重算原始日志与汇总，确认 6 个源文件的 paired delta、后台两个二进制 SHA 相同、16 组 2,703 passed / 0 failed / 8 ignored、4 项 manual PTY 通过。30 条首历史链路独立重算了 send/applied/frame/delivery/ACK/present，session/cursor/sequence 均匹配。

2048-turn default-off 末屏普通 552.8→428.6ms、慢输出 668.5→565.8ms，前后范围不重叠；三次本地测量仍不作统计显著性承诺。普通 512-turn 末屏 173.2→174.0ms 无收益，但 ready prompt 217.9→201.9ms 是另一个观察点，不能混淆。128-turn 末屏也未提升。

普通 2048 首帧产生 359.6→379.5ms，慢输出 276.3→284.6ms，均没有提速。普通首个 PTY sequence 从 1120/1136/1168 变为 2048，与更快消费、同样绘制节奏下更大首帧工作一致；不能据此证明该历史 cursor 的源行可见。保持当前节奏，不把帧调度优化混入本项。

Micro 图片重复引用 196.3→196.8ms、斜杠密集 39.3→40.2ms 均略慢且无收益，不外推 plain 36.5→15.5ms 的成绩。warmup run 0 未纳入正式汇总。RPC 同一二进制的小差异视为运行波动；CPU stage 的嵌套/跨线程时间不相加，不与不同 fixture 的 PTY 时间相减。

结论：本项收益集中在长纯文本的无效图片扫描与 source 临时复制，接受小场景和候选密集文本无明显收益、首帧未改善的边界。恢复前存储解码、正常候选解析、帧生成和慢 writer 仍有成本，后续需独立 change，不在本项追加。详见 [验证记录](verification.md)。
