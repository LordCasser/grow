## MODIFIED Requirements

### Requirement: Pager log dispatch preserves initialized ownership
Pager log forwarding SHALL use its successfully initialized transport and runtime independently of the calling thread. It SHALL retain no more than 256 pending entries or 1 MiB of their encoded content, reject an entry whose encoded content exceeds 64 KiB before materializing a full encoding, and forward accepted entries in FIFO batches no larger than 16 entries or 256 KiB through one sender. Flush before initialization SHALL preserve accepted buffered entries within these limits. Repeated initialization SHALL not create another sender. New over-budget entries SHALL be dropped without displacing accepted entries or affecting session state.

#### Scenario: Plain thread dispatch
- **WHEN** a thread without an entered Tokio runtime emits a flushable log batch after initialization
- **THEN** forwarding uses the initialization runtime instead of discarding the batch because the producer lacks a runtime.

#### Scenario: Flush before initialization
- **WHEN** buffered entries exist and a flush is requested before sender initialization
- **THEN** accepted entries remain available to the eventual initialized forwarder within the pending budget.

#### Scenario: Repeated initialization
- **WHEN** initialization is called after a forwarder is already installed
- **THEN** the existing owner is retained without starting another sender.

#### Scenario: Slow peer or oversized entry
- **WHEN** producers exceed a pending count/byte budget before initialization or while the peer holds an acknowledgement, or submit one oversized encoded entry
- **THEN** new over-budget entries are dropped while accepted entries retain their order; pending plus one in-flight batch remains bounded.

### Requirement: Pager log flush has a bounded delivery wait
An initialized Pager `flush_blocking` SHALL wait for all entries accepted before the call to settle through its one sender, including earlier batches already removed from the pending queue. Its total local wait SHALL stop after two seconds if ACP acknowledgement has not completed. Acknowledged, failed or timed-out sends SHALL not be retried by this forwarder; settlement SHALL NOT claim remote persistence or cancellation of already enqueued remote processing.

#### Scenario: Peer retains acknowledgement
- **WHEN** the peer holds a log notification without acknowledging it, including one dispatched before `flush_blocking`
- **THEN** the flush releases its local wait after the two-second budget without treating the earlier batch as delivered.

#### Scenario: Peer acknowledges or disconnects
- **WHEN** all prior accepted log batches are acknowledged or their channel fails
- **THEN** flush completes without waiting for the deadline.

#### Scenario: Earlier batch remains in flight
- **WHEN** a count-triggered or periodic batch is in flight and another entry is accepted before shutdown flush
- **THEN** flush does not complete until the earlier batch and the later accepted entry both settle, unless its two-second wait expires.

#### Scenario: Concurrent flushes
- **WHEN** two callers flush the same accepted frontier while its batch awaits acknowledgement
- **THEN** both wait for the same settlement without duplicating the batch.
