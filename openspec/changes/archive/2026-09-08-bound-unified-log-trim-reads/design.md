# Design
After acquiring trim lock, obtain ordinary-file metadata length. Read from max(len/2, len-MAX_SIZE/2), at most len-start bytes using Read::take. This bounds payload allocation and excludes concurrent append growth from the read. Retain data after first newline, rewrite and truncate as before. Files without a newline in admitted window remain unchanged.

Small-file behavior stays roughly half; very oversized files retain at most 2.5 MiB. Test a counted seekable reader for exact bytes and huge actual file result, plus existing inode/open-handle/sibling-trimmer tests. No coordination with unlocked appends, crash atomicity, overall snapshot/record-size budget or hard disk cap added.
