# Design

Leave `minimal_transcript_pager_restore_no_artifacts` ignored. The case requires a configured mock provider, submits a turn, opens `/transcript` with `PAGER=less`, quits the pager, and verifies the restored idle screen. On this host the targeted run reached the minimal idle prompt but generated no requests at the mock server and timed out before opening the transcript pager. Therefore neither the pager-child exit nor terminal restore was exercised. Preserve the coverage debt until a bounded test setup produces a passing real-child run.
