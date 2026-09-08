## Why
Unified log trimming reads the entire file before retaining half. The 5 MiB maintenance threshold does not cap memory used when an existing file is much larger.

## What Changes
Seek directly to the later of the midpoint and the final 2.5 MiB; read only that bounded window before choosing the first newline. Preserve inode and exclusive trimmer locking.
