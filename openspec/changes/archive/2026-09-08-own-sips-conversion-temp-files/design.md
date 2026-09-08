## Design
Create the private TempDir in convert_via_sips, then pass ownership to a private conversion helper alongside the command. Fixed child names are safe within this unique directory. The helper retains existing sips arguments, detached/null stdio, conversion status and output read behavior; remove manual per-file cleanup. Dependency tempfile already exists.
## Validation
Inject harmless local commands and owned directories into the same helper to exercise source creation failure, nonexistent executable, nonzero exit after output creation, missing output, successful output and cleanup. Verify independent directory identities and Unix permissions. Existing JPEG conversion tests cover normal backend behavior.
## Limits
RAII cleanup applies to ordinary returns/unwinding, not process termination or filesystem refusal to remove files. Execution timeout, output size and pixel budget are distinct; the existing pixel guard remains, timeout/output work stays separately backlogged.
