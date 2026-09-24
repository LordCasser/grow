# Design

The worker that calls arboard owns a process-local image permit from before `Clipboard::new()` until image read and PNG encode return or unwind. The caller tries admission without waiting. Once the 2-second caller deadline expires, the still-running worker retains the permit; a later paste can fall back to the Linux CLI path or fail on Windows. A dropped receiver does not release the permit.

An arboard image may already have been allocated by the platform before its dimensions are returned. The 16-million-pixel check happens immediately afterward, before Rust PNG output allocation and encoder work; it is not claimed as an OS allocation cap. PNG output uses a writer that accepts at most 50 million bytes. This matches the encoded image clipboard allowance used on macOS.

Linux CLI image capture adds an optional byte limit only for `read_png`, preserving text behavior. The reader stops at allowance plus one byte and closes the pipe; the child is still killed/reaped by the existing deadline if it does not exit. Overflow is an explicit error and cannot be mistaken for absent clipboard content.
