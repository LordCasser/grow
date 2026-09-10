## ADDED Requirements

### Requirement: Windows peer publication coexists with manifest readers
Windows peer heartbeat publication SHALL atomically replace the private manifest while an already-open discovery reader retains the previous file. Owner-only access, no-reparse validation and long-path support SHALL remain enforced; publication failure SHALL preserve the prior manifest.

#### Scenario: Reader spans heartbeat replacement
- **WHEN** a discovery reader holds a peer manifest open while a new heartbeat is published
- **THEN** that reader can finish reading the previous complete manifest, new readers see the replacement, and publication does not first delete the destination or relax its ACL.
