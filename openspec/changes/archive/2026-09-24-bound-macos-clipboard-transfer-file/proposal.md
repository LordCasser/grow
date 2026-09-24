# Bound macOS clipboard transfer files

## Why

Grow bounds how many bytes it reads from the private clipboard transfer file, but AppleScript can write the entire clipboard image to disk before Grow starts that bounded read. A very large pasteboard image can therefore consume unbounded per-file temporary storage.

## What Changes

Set a child-local `RLIMIT_FSIZE` of 50,000,000 bytes for macOS clipboard AppleScript subprocesses. If the helper attempts to write a larger transfer file, the script must fail through the existing subprocess error path and owned private temporary directory cleanup.

This limits each regular file written by the subprocess tree. It does not limit helper/AppKit RSS, aggregate filesystem use across multiple files, or image decode memory.
