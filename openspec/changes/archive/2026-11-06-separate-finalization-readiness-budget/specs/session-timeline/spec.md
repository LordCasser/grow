## ADDED Requirements

### Requirement: Finalization readiness does not consume reconciliation attempts

Within an existing finalization period, the Coordinator SHALL observe host readiness before admitting a reconciliation attempt. Observations that the original output I/O or complete child actor/persistence incarnation still owns publication, or that no healthy effective parent writer is available, SHALL NOT consume the three-attempt budget. These observations SHALL NOT renew the period's original absolute deadline, acquire replacement writer authority or dispatch recovery mutations. Observing the existing incarnation MAY retain its existing request to stop the persistence rail after its actor exits; that retirement signal SHALL NOT grant recovery write authority. Once ready, reconciliation SHALL retain existing lease, identity, exact-ACK, retry and deadline checks. An admitted attempt still consumes its budget if a later race or operation fails. Read-only status/wait SHALL NOT open another period.

#### Scenario: Owner retires after several observations

- **WHEN** more than three readiness observations in one period find the old writer alive and the writer retires before that period's original deadline
- **THEN** no reconciliation was dispatched during those observations, an attempt can proceed after retirement, and the original deadline remains unchanged.

#### Scenario: Owner retires after the deadline

- **WHEN** readiness becomes true only after the original period deadline
- **THEN** no new reconciliation is dispatched until the existing explicit next-turn recovery period is admitted; ordinary observation does not reopen it.

#### Scenario: Admitted attempts fail

- **WHEN** the host is ready and three admitted reconciliation attempts fail
- **THEN** the same period dispatches no fourth attempt and preserves the pending child responsibility and unknown operations.
