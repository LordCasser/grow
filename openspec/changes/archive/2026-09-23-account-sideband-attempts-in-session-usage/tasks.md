# Tasks

- [x] Add durable started/settled Sideband attempt observations and restore validation to the owning session ledger.
- [x] Fold known auxiliary usage and unknown attempts without increasing main-loop turn count or double-counting child bills.
- [x] Wire every Sideband provider path, including retries, compaction, cancellation and owner drop, through acknowledged settlement.
- [x] Cover live/recovered `/usage`, headless, Goal consistency, duplicate/conflicting settlement and crash windows with focused tests.
- [x] Update developer documentation, remove the resolved backlog item, and validate the change before archiving.
