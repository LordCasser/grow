## Why

`client-support::placeholder_images` still exposes an orphan-recovery loader that canonicalizes a textual path, checks an allowlist and metadata, then opens that pathname. Replacement between those steps can change the opened file. Repository-wide caller inspection finds no production caller of the loader or orphan recovery; Pager and Shell already strip placeholder paths and accept image bytes only through explicit attachments. Keeping the unused reader creates an avoidable filesystem access surface, while the archived client-surfaces spec and developer guide still describe it as active.

## What Changes

- Remove the unused implicit placeholder file loader, recovery API, associated parsing/allowlist machinery and tests.
- Keep visible `[Image #N]` anchors, explicit image attachments, and file URI encoding/decoding helpers.
- Replace obsolete recovery requirements with a contract that textual placeholders alone never load a file.

## Impact

`client-support` no longer offers the unused public recovery functions. No in-repository production caller changes. This intentionally removes the direct Rust API rather than maintaining a race-prone path under a misleading security claim.
