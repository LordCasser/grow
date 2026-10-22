# Design

## 复用现有 owner

`load_session_for_write_without_updates` 已先取得 writer lease，同一 adapter 随后交给 persistence actor。沿此路径初始化已有 `LedgerPrefix`；不增加 transferable token、全局 registry、另一份 Timeline 或新 actor。

本项仅在 Unix 复用带 dev/inode/ctime 的原 stamp；其他平台保留原全量 prefix 验证，不凭不足的身份条件采用快路。初次读取绑定 pinned directory 和具体文件，在既有 append 锁保护的同步阶段捕获前缀、读取完整记录、累计原始字节 hash 并做私有 fold；读前/后文件身份与 stamp 必须一致。全部必要 artifact/Sideband 校验成功后才使该 prefix 可用于 writer。不得在持有同步锁时等待 actor、gateway 或可重入 append；首次 load 不修复只读历史。初次验证产生的 Timeline 沿已有返回路径供 runtime 使用，避免为 prefix 再建一份。

缓存安装不得覆盖本 adapter 已推进的更晚 prefix；load 内任何合法补写必须走原 append，推进同一 prefix。首次 append 仍在既有锁下重新检查当前文件。状态不足或平台不能提供原有核对条件时采用原全量验证，不以相同 seq/长度代替字节证明。

## 对抗性核对

| 边界 | 必须保持 |
| --- | --- |
| 同长度改写、替换 inode、truncate、读中变更 | 读中变化拒绝；append 发现变化走既有完整验证/hash 比较或拒绝。不能把当前 metadata 贴到先前读出的旧 hash 上。 |
| prefix 已计算但 artifact/lifecycle 校验失败 | 不发布可用 runtime 或 prefix；清除本次候选，不能污染下次 load。 |
| 路线补写、同 adapter 重试、失去 lease | 自身补写增量推进；不覆盖已确认状态；新 epoch 重新验证。 |
| 未换行尾片 | hash/长度只覆盖完整前缀；尾片处理保留原规则，不把截尾后的 stamp 当作未验证正文的证明。 |
| file/directory sync 失败或 ACK 丢失 | 可读的 exact tail 不等于 durable，重试仍同步原记录；不重复提交或重采样。 |
| 只读或独立 writer | 不从它们借用 prefix；观察不取 writer lease、不修复文件。 |

stamp 是现有独占 writer epoch 内的失效检测，不是能够抵抗任意修改者的内容认证。此 change 不削弱现有校验边界，也不承诺文件系统原先不提供的保证。锁覆盖读取会增加初次恢复临界区时长；应限制在必要本地 I/O，不把其他 session 或网络操作纳入。

## 理论收益

健康冷恢复省掉一次完整 Timeline 读取/typed decode/fold，渐近仍为 O(历史量)。原始 hash 可在第一次必要读取时顺手计算，代价是一次线性字节处理；主要收益来自避免第二次解析与状态构造。峰值减少一次临时事件集合；磁盘体积与正常 append 的 durable 保证不变。

R6 同轮 debug 的 actor call 为 1744 ms，包含 prefix 之外的启动工作，不能把它全部当作节省；C1 release 已测 prefix 独占中位数 84.707 ms。首条历史 6.69 s 的 seeded-route 样本中这次 writer 初始化发生在初次 replay 之后，所以本项主要改善 ready，不能宣称同幅改善首屏。测量必须单独标记 prefix 读取次数及耗时。

入口：`crates/codegen/shell/src/session/storage/jsonl/mod.rs::{load_session_for_write_without_updates,validated_timeline,timeline_prefix_state,append_timeline_line_in_directory_sync_with_probe,load_timeline_prefix}`。复用 `LedgerPrefix`、`LedgerFileStamp`；变动控制在这些边界及必要调用方。
