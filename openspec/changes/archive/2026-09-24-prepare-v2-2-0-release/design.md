# Design

Keep the workspace package version as the single source of truth. Update only the corresponding local package entries in `Cargo.lock`; do not re-resolve dependencies. The release notes use absolute links because GitHub renders them outside the repository path. The changelog index links to the detailed notes for local readers.
