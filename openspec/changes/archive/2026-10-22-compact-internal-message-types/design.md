# Design

先缩小造成大枚举的内层 payload，再根据编译诊断处理剩余分支，避免按旧尺寸盲目 Box 所有变体。Box 由原 owner 独占，不新增共享状态、延迟提交或异步生命周期；转换后消费点显式解包，保持 send/ACK/cancellation 的顺序。serde/schemars 对标准 Box 保持原透明结构，必须由已有 DTO golden、Timeline round-trip、恢复/持久化和子会话测试验证，不能仅假定编译通过。

复杂 future 优先用现有 LocalBoxFuture/SendBoxFuture；数据集合用局部类型别名表明意义，不加包装层和工具框架。错误只在必要的边界间接承载，避免为了 lint 把标准库所有权返回改为丢弃。每个新增分配点记录其代价，不承诺 resume 加速。保留全量恢复与就绪边界，不做分页分离、SQLite 或存储格式变更。

固定 1.93.1、独立 HOME/GROW_HOME 和共享的本轮专用 target。通过 fmt、workspace all-targets/默认 CLI check、Clippy 重扫；执行受影响库、DTO/协议及关键恢复用例，核对错误路径/发送失败/子会话归属/顺序。必要的 release 恢复回归复用已有现代 fixture，结果只作为回归依据。

局部 lint 例外：Axum 的 Host 拒绝保留框架响应 tuple；DraftCapture 保留高频草稿快照内联；JournalFact/AppendFact 是逐条解码、提交的临时值，不是累积保存的历史槽，因此保留 inline，避免给每条 presentation 额外分配。这些使用带理由的 expect，若诊断消失会触发未满足期望，不能掩盖其他诊断。
