## 1. Snapshot Size Contract

- [x] 1.1 Add the client-surfaces delta describing the shared 1 MiB write allowance and rejection behavior; verify scenarios match the loader and controller flow.
- [x] 1.2 Enforce the limit before asynchronous writer initialization/queue handoff and before filesystem publication; verify with boundary and destination-preservation tests.
- [x] 1.3 Add a real `SlashController::record_command_use` test proving an oversized command is rejected and the persistent store remains dirty; verify it performs no filesystem publication.

## 2. Documentation and Validation

- [x] 2.1 Update the developer MRU persistence note to state the encoded write limit and rejection behavior; verify it links the archived contract.
- [x] 2.2 Run the focused pager tests and strict OpenSpec validation; record results in verification.md.
- [x] 2.3 Archive the completed change and verify the archived contract and change archive pass OpenSpec validation.
