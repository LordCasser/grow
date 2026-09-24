## Decision

Treat placeholder text as a display anchor, not an instruction to open a file. The existing Pager and Shell admission paths strip the path portion from text; actual image content arrives as a separate image attachment. Remove the unused recovery implementation instead of adding descriptor-relative filesystem machinery to code that has no production caller.

## Boundaries

Preserve `strip_paths_from_image_placeholders`, `file_uri_from_path` and `canonical_from_file_uri`. Preserve the numbered anchor and attachment URI behavior. The retired direct Rust recovery API is not retained for compatibility, consistent with the project's no-backward-compatibility rule. URI parsing is not itself an image read.

## Verification

Check every in-repository callsite, run focused client-support tests and a workspace type check, and validate OpenSpec before and after archive. Existing sanitizer tests cover numbered anchors, malformed text and attachment URI round trips; a code search confirms no path-derived image read remains in this module.
