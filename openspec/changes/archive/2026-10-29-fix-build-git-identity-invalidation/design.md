## Design

Both build scripts include a small shared helper from the `version` crate directory. The helper asks Git for its per-worktree and common metadata directories, watches the worktree `HEAD`, and watches `packed-refs` when it exists. When `HEAD` names a branch, it watches the loose ref when present; for a packed ref it watches the nearest existing ancestor directory of the ref path so a later loose update invalidates the build, including nested refs whose intermediate directory was pruned. Missing files are not emitted as Cargo rerun paths because Cargo repeatedly rebuilds for nonexistent paths. A detached HEAD has no branch ref to watch; its `HEAD` file carries the object ID.

Git discovery remains optional: outside a Git checkout the helper returns no paths, preserving the existing `unknown` commit fallback. The CLI's explicit release commit handling is untouched.

No archived behavior contract is changed; `.openspec.yaml` uses `skip_specs: true`.
