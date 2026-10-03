## Release identity

Keep the existing patch line at 2.3.1. Use the workspace version in `Cargo.toml`, regenerate the corresponding workspace entries in `Cargo.lock`, and add `crates/codegen/shell/changelogs/2.3.1.md` with an absolute comparison link from v2.3.0.

## Sequence

The replay and ordered-resume behavior changes and their OpenSpec records are already committed and archived. Prepare version metadata and notes, run the repository's strict OpenSpec and release preflight checks, commit the release preparation, create an annotated `v2.3.1` tag at that commit, push `main` and the tag, then dispatch the official all-platform publication workflow.
