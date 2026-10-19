## MODIFIED Requirements

### Requirement: Image recovery is one visible operation row

When a text-only model requires image recovery, the client SHALL render the recovery lifecycle as one ordinary operation row with a stable operation identity, not as a NOTICE or a fabricated model tool call. The row SHALL show a running indicator while the configured vision model is called, update in place when local OCR begins, and reach a clear English terminal state for success, partial success, failure, or cancellation. Repeated updates and session replay SHALL update or reconstruct that same row. Cursor reconnect SHALL merge a repeated operation into the existing row while preserving its EntryId. A terminal row SHALL not retain a running indicator or be reopened by late progress.

#### Scenario: Vision recovery succeeds
- **WHEN** the configured vision model converts one image
- **THEN** the running row settles in place to `Converted 1 image`.

#### Scenario: OCR follows vision recovery
- **WHEN** vision recovery fails or is unavailable and local OCR starts
- **THEN** the existing row changes to show OCR progress and then settles to the final result, without adding a second progress row.

#### Scenario: Recovery is partial or fails
- **WHEN** only some image groups are converted, or no recovery source succeeds
- **THEN** the same row reports the converted and unconverted counts, or a clear failure state, in English.

#### Scenario: Recovery is cancelled or replayed
- **WHEN** the owning operation is cancelled or its persisted lifecycle is replayed
- **THEN** the row reaches a terminal state when the owning turn is terminal, while a genuinely active reconnect remains running; replay never creates duplicate rows for updates sharing the operation identity.

#### Scenario: Offline snapshot ends during recovery
- **WHEN** an offline transcript snapshot ends without a terminal event for a running recovery operation
- **THEN** the read-only projection stops its running indicator and labels the row `Image recovery incomplete at snapshot`, without changing persisted history or claiming that the operation failed or was cancelled.

#### Scenario: Driver RPC wins the terminal race
- **WHEN** the driver's prompt response arrives before the durable turn notification
- **THEN** the shared exact-prompt terminal boundary settles that prompt's operation once; a stale terminal for another prompt has no effect.

#### Scenario: Resident reconnect restores an active conversion
- **WHEN** reconnect finishes with an authoritative active foreground prompt
- **THEN** only that prompt's unfinished conversion remains animated after generic replay cleanup; older unfinished rows stop, and a failed reload labels unresolved conversion status unavailable.

#### Scenario: Cursor reconnect merges a live completion
- **WHEN** cursor reload 前 scrollback 已有某 operation 的 Started 行，reload 期间该 operation 收到 Completed，随后 cursor tail 合并成功
- **THEN** 最终 scrollback 仅有该 operation 的一行，保留 reload 前行的 EntryId，并显示 Completed 内容且不再运行。

#### Scenario: Terminal recovery ignores late progress during merge
- **WHEN** cursor reconnect 合并的旧行已是终态，而 tail 对同一 operation 含迟到的 running progress
- **THEN** 保留原 EntryId 与终态内容，行保持非运行，且不重复输出已提交的行。
