## ADDED Requirements

### Requirement: Kitty keyboard teardown has an owned reply fence

Grow 在正常终端退出中 SHALL 先确认输入 reader 不再消费 TTY，并收束已接受的渲染输出，然后发出已推入 Kitty keyboard flags 的 pop。仅当确实推入过 flags、输出可写且当前进程独占 TTY 输入时，SHALL 在仍为 raw mode 的状态下向该终端发送 DA1 查询，有界消费输入直至收到完整 DA1 回复，再关闭 raw mode。fence 的超时或 I/O 错误 SHALL 不阻止 best-effort 终端恢复；不得通过与存活 reader 并发读取来假装完成屏障。

#### Scenario: Late Kitty release on normal quit
- **WHEN** flags 已推入、用户正常退出，终端在旧 10 ms drain 窗口之后、DA1 回复之前发送 keyboard release
- **THEN** writer 帧先于 teardown，pop 先于 DA1；fence 消费 release 并在完整 DA1 后恢复 raw mode，release 不留给 shell。

#### Scenario: Silent or malformed DA1
- **WHEN** 终端不回复、只回复部分序列，或回复 DA2/CSI-u 而非完整 DA1
- **THEN** 不将其判为 fence 完成；到固定有限期限后仍关闭 raw mode、显示光标并返回，不无限等待。

#### Scenario: No Kitty flags
- **WHEN** 会话未成功推入 Kitty keyboard flags
- **THEN** 退出不发 pop 或 DA1，沿既有恢复路径结束。

#### Scenario: Input or output ownership cannot be established
- **WHEN** reader 停止确认超时、writer 无法安全收束、查询写入失败，或没有可用 TTY
- **THEN** 不与 reader 并发读 stdin，不执行不可靠的回复读取；仍执行可行的终端恢复并记录降级原因，退出不因 fence 增加无界等待。

#### Scenario: Forced exit remains fast
- **WHEN** 首次受控退出信号转为正常 quit，或 panic/第二次信号走强制退出
- **THEN** 前者在条件满足时经过正常 fence；后者只做快速 best-effort teardown，不等待 reader、writer 或 DA1。
