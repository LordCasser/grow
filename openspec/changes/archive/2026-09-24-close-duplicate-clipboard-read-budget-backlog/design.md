# Design

Treat the backlog item as duplicate only after accounting for every named boundary:

- AppleScript execution, output, and process-group cleanup already have an explicit process contract.
- macOS encoded clipboard bytes are bounded before Rust allocation/copy, and temporary image files are read with a limit-plus-one reader capped at 50,000,000 accepted bytes.
- macOS native AppKit initialization and locked native messaging, including the content read, remain covered by the separate clipboard-native-call deadline audit.
- Non-macOS RGBA acquisition, PNG encoding peak memory, and pixel limits remain covered by the image system-memory budget item.
- Prompt path-image input has a 50,000,000-byte per-image cap and a 50,000,000-byte aggregate cap. The shared dimension probe parses headers with full pixel decoding disabled.

A synchronous local temporary-file read has no safe cancellation point. Adding a deadline without transferring the read to an owned worker would only return early while the read continued in an unowned thread. That would require a separate lifecycle/resource design, so this entry does not claim a deadline guarantee. No specification delta is needed.
