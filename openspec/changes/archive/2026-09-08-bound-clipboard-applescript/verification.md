## Verification
Three production AppleScript callers use run_clipboard_script with SCRIPT_TIMEOUT=5s. Existing builder still detaches the command and passes paths through argv. The runner captures each pipe with a limit+1 reader, checks a shared overflow signal while waiting, kills the owned group even after leader success, and bounds subsequent channel collection to one shared 300ms deadline. Group attachment failure kills/reaps the direct child. Cleanup errors are not success.

Initial isolated tests: 1 passed, 1 failed. Flood test observed EPERM from process-group cleanup, which masked the output-limit error. Changed error composition to retain both primary and cleanup failure; no permission error is ignored except ESRCH (group gone). Subsequent process boundary tests: 2 passed (1.40s).

Final `cargo test --locked --offline -p client-support --lib clipboard --quiet`: 51 passed, 4 ignored, 0 failed (1.41s). Includes exact 1MiB stdout, stdout/stderr flood, nonzero status and intact stdout/stderr, hung leader, leader exit with descendant-held pipes and delayed marker absence. Existing path argv roundtrips and AppleScript syntax tests included. Real clipboard tests stayed ignored. git diff --check passed.

The tests do not prove reclamation if the OS refuses kill or descendants escape the owned group; failures are reported. They also do not bound native AppKit, pbpaste, image-file bytes or image decoding. These remain in backlog, outside this change. The only temporary files in tests are isolated marker files; no real clipboard contents were modified.
