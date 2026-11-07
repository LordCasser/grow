## ADDED Requirements

### Requirement: Crash captures retain per-process file ownership

When crash capture is enabled, each installation SHALL exclusively create its own private raw file and retain an OS-backed claim on the exact capture handle until its final write or process retirement. Another process's startup SHALL NOT truncate, read, consume or remove a claimed raw file. Recovery SHALL obtain an exclusive nonblocking claim before reading or consuming evidence, serialize report publication and retention, and distinguish crashes captured in the same second. Raw evidence SHALL remain intact until the existing report and history durability boundaries succeed. Existing raw evidence from the former single-file layout SHALL remain recoverable.

#### Scenario: Another process starts while capture is live
- **WHEN** A holds a live capture and B starts capture or recovery in the same directory
- **THEN** B uses a distinct capture file and leaves A's bytes and file identity untouched.

#### Scenario: A captured process dies
- **WHEN** A's capture owner retires and C successfully claims its nonempty raw file
- **THEN** C publishes that crash's report and unique history before consuming only the claimed raw file.

#### Scenario: Concurrent recovery and same-second crashes
- **WHEN** multiple recoverers observe two completed captures with the same timestamp
- **THEN** only a claim holder consumes each capture, publication/retention is serialized, and the two crash histories cannot overwrite one another by timestamp alone.

#### Scenario: Empty or failed capture recovery
- **WHEN** recovery sees a claimed empty capture or encounters read, parse or publication failure
- **THEN** a live capture remains untouched; only a successfully claimed inactive empty file may be removed, and nonempty failed evidence remains available with an observable error.
