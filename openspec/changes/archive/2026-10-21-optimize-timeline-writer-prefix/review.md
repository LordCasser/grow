# 对抗性核对

审计比较 Timeline::from_events / accept_replayed 与 live accept。前者在私有 fold 中依次验证 header、content、lifecycle；后者先验证 lifecycle 的副本再验证 content。有效序列的最终状态一致，无效序列均失败，不承诺同时违反两项约束时的首条错误文本相同。沿用 chat-state 已有 bulk/live 一致性与 live 失败原子性回归。

writer前缀仍读取 complete_len 内的原始字节并校验 newline/record上限，hash不是重序列化事件的hash。新增语义非法但JSON完整记录回归，断言 InvalidData 且文件字节不变。已有durable_tests覆盖短文件、空行、超限、含CRLF/空格的原始hash与hasher续接、未提交尾、同长度内部修改、替换及sync失败重试。

算法修改只位于load_timeline_prefix，append lock、stamp/hash比对、tail seq/payload和file/directory sync未改。短时多一个拥有事件槽位的Vec，不增加整份JSON或正文深复制。

性能运行串行；构建不与本任务计时重叠。宿主上发现另一个项目jarde的release编译，未干预其他任务；各轮保留原始样本和范围，使用三次中位数，不能视为隔离实验室结果。
