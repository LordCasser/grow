# Design

Add one private error predicate in the JSONL adapter: only NotFound and InvalidData indicate a skippable candidate. Apply it to cwd/session directory opens, initial summary reads, and physical identity validation (which can read a long-CWD marker). Propagate all other errors unchanged. Hidden/current-format/cwd filtering stays after physical validation. Existing contained-directory opens normalize symlink/non-directory rejection to InvalidData. No new scan-result structure or retry mechanism is needed.

The scan builds results before returning. cleanup_stale_sessions_sync evaluates the complete result before its deletion loop; reindex_all obtains the complete summary list before indexing/pruning/completion. Thus failure follows existing caller handling. Corrupt summaries continue to be excluded as before; this change does not alter their projection policy or claim every excluded entry is absent.

Test real unreadable cwd directories, session directories, summaries and long-CWD markers using temporary paths and restore permissions before assertions. Include another expired readable session to prove cleanup does not delete it after incomplete discovery. Permission tests require a non-root Unix host; document platform limits. Existing invalid/symlink/filter and cleanup tests protect intentional skipping.
