# Verification

## Evidence and change
Before this change both dispatch_open_transcript_pager and minimal full_view::finish_transcript used std::fs::write on a generated temp path. AppView stored a bare PathBuf; only the successful event-loop path called remove_file. Replacement and non-timeout error paths had no owner cleanup.

Both producers now use write_pager_transcript, which creates NamedTempFile before writing and returns TempPath after a complete write. The pending field owns TempPath. The event loop borrows it for the child, moves it back on timeout, and drops it on completion or return through an error. This follows the existing request handoff rather than adding a cleanup service.

## Checks
- pager --lib pager_transcript: 3 passed. Tests read actual Markdown/ANSI files, assert Unix 0600, verify old path removal on replacement and pending removal on AppView drop, inject a partial write error and check the directory is empty, and move a real owned file through suspend retry before dropping it.
- First test compile failed because the new fixture used nonexistent ScreenMode::default and RenderBlock::user_message; corrected to Fullscreen and user_prompt. No production failure was inferred from that fixture error.
- Existing macOS linker compact-unwind-size warning remains.
- pager-minimal --lib full_view: 8 passed; compiles the updated minimal call site and retains full transcript rendering regressions.
- git diff --check passed.
- OpenSpec all strict validation before archive: 16 passed.

## Limits
The non-timeout suspend-error cleanup is established by the local owner and Rust return/drop control flow plus owner-drop tests; this run does not inject a real terminal failure. No PTY end-to-end run or CLI relink is claimed. SIGKILL, process abort, power loss, or deletion errors do not guarantee cleanup. Rendering and snapshot writes remain synchronous; responsiveness is separately recorded in backlog. The existing uuid dependency remains used by a minimal rendering test fixture.
