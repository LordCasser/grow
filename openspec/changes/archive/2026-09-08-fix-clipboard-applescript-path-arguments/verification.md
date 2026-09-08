# Verification

## Implementation
Two image-read bodies previously interpolated PNG/TIFF/JPEG paths into quoted AppleScript source; set_image_file escaped only double quotes and left backslashes unhandled. All three now use clipboard_osascript_command, which wraps the body in on run argv and supplies paths as separate arguments after --. Scripts use POSIX file (item N of argv). No path remains in script source construction. Temporary directory ownership, format priority and exit-status handling remain.

## Actual tests
Using CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216:
- cargo test --locked --offline -p client-support --lib clipboard_osascript_ --quiet: 2 passed. Real osascript returns each path argument unchanged for spaces, quotes/backslashes, newline, Unicode, leading option syntax and script-like text. Real osacompile compiles the generated attachment body without executing it.
- cargo test --locked --offline -p client-support --lib attachments --quiet: 17 passed.
- cargo test --locked --offline -p client-support --lib clipboard_probe_ --quiet: 2 passed (isolated private directory and error cleanup).

An initial compile exposed a missing argument at the attachment runner call; it was corrected before the successful test runs. New AppleScript tests do not access or mutate the clipboard. No real image paste/copy or installed binary update was performed. Subprocess execution deadlines and payload limits remain separate work.
