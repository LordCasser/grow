## ADDED Requirements

### Requirement: Nonrestorable initialization cancellation releases waiters
不可重建的 stdio 初始化被取消时，在状态锁可取得且仍为 Initializing 的情况下，取消守卫 SHALL 将状态置为 Empty 并通知等待者；不可重建 SHALL 不被当作守卫已撤销。

#### Scenario: stdio 握手取消
- **WHEN** stdio 握手等待响应时取消，状态锁可取得
- **THEN** 状态变为 Empty、等待者被通知，后续初始化立即返回无 transport 错误。
