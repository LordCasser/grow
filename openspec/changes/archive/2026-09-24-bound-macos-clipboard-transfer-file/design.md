# Design

Apply `RLIMIT_FSIZE` in the `pre_exec` hook for the owned `osascript` child, lowering both soft and hard limits to at most 50,000,000 bytes. Descendants inherit the limit, so the AppleScript file write cannot extend a transfer file beyond the existing encoded-data allowance. File-size overflow causes the helper write to fail; the existing nonzero-process-result path returns an error, and `get_image` / `get_attachments` remove their private per-call transfer directory on the error path.

The AppleScript catches errors only around each clipboard-to-image-type coercion so unsupported PNG/TIFF/JPEG types can fall through to the next format. File open, write, and close operations run after the coercion handler; their errors escape the script and produce a nonzero `osascript` result. The unified attachments script keeps its existing file-URL-first gate and skips image coercions entirely when file URLs are found.

The limit is per regular file and applies to all owned clipboard AppleScript subprocesses. It does not cap total temporary-directory usage if a script writes multiple files, the helper's memory, kernel cache, or decoding performed later. In particular, it is not an RSS limit for osascript or AppKit.
