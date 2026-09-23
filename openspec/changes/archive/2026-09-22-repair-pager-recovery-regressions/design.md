## Context

`replay_inherited_updates` 通过 `stream_replay_updates_at` 进入 `with_reconciled_replay_lines`。后者读取同一已打开会话的 Timeline 并 fold 校验，然后核对 updates；`OpenedSession::timeline_events` 将缺失 ledger 明确报告为 InvalidData。旧测试只写 Summary/updates，因此正常恢复分支没有执行。

`updates.jsonl` 的普通 ACP 工具展示仍由工具 runtime 投影持有；本次这些无 response admission identity 的夹具可以使用合法的空 Timeline，无需伪造 provider response 或为每个显示 chunk 编造模型消息。真正需要 Spawn/receipt 的用例保留实际 Timeline event。

## Decisions

- 在现有测试帮助函数及少量独立磁盘夹具中创建 Timeline，不引入新生产 helper 或存储适配层。
- 空历史场景必须是合法 Summary、空 Timeline、空 updates，不能让损坏 Summary 冒充空回放。
- 首条输入测试显式设置当前线程的 `combine_queued_prompts` cache 并恢复原值。关闭合并时 directive 先发送，后续输入仍排队；开启时一次发送按 FIFO 合并的两个 segment。断言同时覆盖 effect、in-flight identity 与 pending 字段清空，不把队列为空直接解释成输入丢失。
- 不降低 reader 校验，不删失败场景，不改已有 response projection change 的实现。

## Verification

保留修复前整包失败记录；本轮隔离 target 编译后重现相关失败，修改后运行定向模块及 Pager 整包。记录工具链、忽略项和验证限制。最后执行严格 OpenSpec 与归档校验，并清理本轮 Rust target。
