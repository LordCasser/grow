## Why
User image batch persistence writes each asset and returns early on any later failure, leaving completed assets from a failed batch with no returned image_files list. Repeated failed admission can accumulate orphaned files.

## What Changes
On batch decode/write error, attempt handle-relative removal of only files successfully created by this call, preserve original error and warn on cleanup failures.
