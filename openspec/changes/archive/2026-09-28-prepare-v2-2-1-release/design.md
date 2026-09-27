# Design

Keep the workspace package version as the single source of truth. Refresh only local workspace entries in `Cargo.lock` with Cargo's offline workspace update, without changing third-party dependency versions. Release notes use absolute links because GitHub renders them outside the repository path; the shell changelog index links to the detailed notes.
