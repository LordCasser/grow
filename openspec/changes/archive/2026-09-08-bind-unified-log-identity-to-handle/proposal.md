## Why
open_writer_at opens a descriptor then separately stats its pathname. Replacement in between associates the new path inode with the old descriptor, defeating subsequent stale-handle detection.

## What Changes
Capture writer identity from metadata of the opened file. Retain pathname identity lookup only for maintenance comparison. Fail writer construction if descriptor metadata cannot be obtained.
