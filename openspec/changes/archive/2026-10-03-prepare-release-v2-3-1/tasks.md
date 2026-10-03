# Tasks

- [x] Confirm the replay and ordered resume-history changes are committed with archived OpenSpec records.
- [x] Update workspace version, lock file, and public 2.3.1 release notes.
- [x] Run strict OpenSpec validation and verify release preflight prerequisites.

After archiving and committing this preparation, create annotated tag `v2.3.1` at the clean release-preparation commit and run `scripts/validate-release.sh v2.3.1` before pushing or dispatching the publication workflow.
