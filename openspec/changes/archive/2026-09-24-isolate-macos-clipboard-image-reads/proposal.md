# Isolate macOS clipboard image reads

## Why

`NSPasteboard dataForType:` materializes an `NSData` before Grow can inspect its length. The current 50,000,000-byte check therefore limits only the later Rust copy and does not protect the Grow process from a large native allocation.

## What Changes

Route explicit macOS image and attachment image reads through the existing `osascript` subprocess and private temporary-file path. Keep the existing 50,000,000-byte bounded read in Grow, and retain AppKit only for metadata probes (`changeCount` and `types`). Remove the native image reader and its environment kill switch.

This adds the existing subprocess/temp-file latency to the unambiguous image paste path. It isolates pasteboard image materialization from the Grow process, but does not impose a memory cap on macOS or `osascript`, nor bound image decode or filesystem/kernel activity.
