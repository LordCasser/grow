## Release identity

Use the workspace's single version at `Cargo.toml`, update `Cargo.lock` through Cargo, and write `crates/codegen/shell/changelogs/2.3.0.md`. Add a short 2.3.0 pointer to the shell changelog. The public notes describe only behavior present in the committed tree and use an absolute GitHub compare link because GitHub Release renders the text outside the repository path.

## Sequence

First finish the source and OpenSpec feature commit and verify the tree. Then prepare version metadata and notes, validate, archive this change, and commit the release preparation. An annotated `v2.3.0` local tag must resolve to that release-preparation commit. `scripts/validate-release.sh` runs only against the clean, tagged tree. Do not dispatch the publication workflow as part of this preparation.
