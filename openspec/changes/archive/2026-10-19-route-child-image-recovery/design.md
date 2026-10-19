# 设计

## 运行时投影

live Grow 通知按 session id 进入 child 分流白名单，再由 child handler 将 `ImageRecoveryOperation` 投影到对应 child 的 scrollback。行更新复用 root 的 `apply_image_recovery_operation`，operation id 保持行身份；root scrollback 不接收 child 行。

child `TurnCompleted` 只收尾 child scrollback 中与 payload prompt id 一致的未完成 recovery。live 通知还必须匹配 child 当前活动 prompt，若 envelope 的 prompt id 存在且与 payload 不一致也丢弃。历史 replay 按持久化顺序处理，因此旧 prompt 的 durable terminal 可关闭其历史 operation。该分支不调用 root 的 turn finalizer。parent 收到 `SubagentFinished` 时，由 child finalizer 收尾 child 中仍运行的 recovery。

## 持久化回放

child transcript 必须保持 ACP 和 Grow 生命周期在 `updates.jsonl` 中的原始顺序。Pager 通过 `stream_session_history_at` 单次读取 reconciled reader；该 reader 保留 ACP / UiNotice 输出，并额外输出 ImageRecoveryOperation 与 TurnCompleted。不要把 ACP transcript 与第二个 Grow-only scan 分开投影，否则会把 recovery 行移到后续 ACP 输出之后。source reader 仍负责 rewind filtering、timeline reconciliation 与 committed-ledger读取。

## 失败与重放

- child route 缺 view 时不产生行；route 用当前 session id 与现有 event highwater 处理 replay/live 重复。
- stale live TurnCompleted 不收尾另一 prompt 的行；晚到 progress 不重新打开已终态 operation。
- 终态处理限定到所属 child；`SubagentFinished` 仅终结对应 child view。
- cursor tail 的同 operation 合并保留原 EntryId 与终态语义，契约和场景累积自 `merge-image-recovery-reconnect-rows`。归档时先归档该 change，再归档本 change。
