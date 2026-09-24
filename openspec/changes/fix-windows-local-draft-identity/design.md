## Approach

Open the active Windows path through `same_file::Handle` and compare it with a handle cloned from the original validated file. A missing active path remains a safe mismatch; any other open error propagates. Keep both handles alive during the comparison. Add the dependency only for Windows because Unix already has a stable device/inode check.

The final quarantine rename retains the existing check-then-rename race; this change only repairs the broken Windows identity check, without expanding the quarantine design.
