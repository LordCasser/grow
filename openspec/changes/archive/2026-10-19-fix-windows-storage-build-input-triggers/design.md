## Context

`.github/workflows/windows-session-storage.yml` has matching, narrow path lists for `pull_request` and `push` to `main`. Unlike `.github/workflows/local-coordination.yml`, those lists currently exclude `.cargo/**` and root `Cargo.toml`. `.cargo/config.toml` sets Windows-target compiler flags, and the root manifest defines the workspace build graph, so either can affect the session-storage tests.

The release workflow calls this reusable workflow with the validated commit SHA. Its `publish` job depends on the Windows regression job and requires its result to be `success`. This change must preserve that path-independent release behavior.

## Decision

Add exactly `.cargo/**` and `Cargo.toml` to both source-trigger path lists. Keep both lists aligned and leave `workflow_call` without path gating. Add a Python regression test that checks the direct build inputs are present in both lists, that the lists agree, and that the release workflow still passes the validated commit to the reusable job and requires its success before publication.

## Risks

The check validates repository configuration statically; it cannot prove GitHub-hosted runner behavior. GitHub workflow trigger semantics and the actual hosted Windows run remain external to this local verification.
