# Isolate unified log writes from callers

## Why

`unified_log::emit` and client log ingestion currently wait for a process mutex, an inode advisory lock, maintenance trimming, and the filesystem append on the caller's thread. A slow filesystem or another process holding the inode lock can therefore block Shell or Pager work for an unbounded period. The 64 KiB record cap limits bytes, not latency.

## What Changes

- Encode bounded records on the caller and hand them to one bounded process-local writer queue without waiting for disk I/O.
- Keep the existing inode coordination, path healing, trim, and record encoding in that worker. On queue saturation, drop diagnostic records with an observable, coalesced loss count; diagnostic logs are not a durable fact source.
- Provide a bounded flush barrier for diagnostic snapshot/normal shutdown. Do not claim that a blocked OS write can be cancelled or that the existing 5 MiB maintenance threshold is a hard disk quota.
