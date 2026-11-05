## Evidence and boundary

The reported checkout's `.git/config` has size 137, zero allocated blocks and macOS flags 1073741920. Direct `read_bytes()` fails with `Operation timed out`. The running pager logged `session.create.done` after 78080 ms; `grow inspect` took 17.3 seconds and sampling/lsof showed a synchronous config read. Directory enumeration and marker metadata reads completed immediately.

`Repository::discover` wraps `discover_path` followed by `Repository::open`. Only the latter needs Git config. Configuration discovery needs the checkout boundary, while trust needs root/marker/common-directory filesystem identities. Neither requires repository settings.

## Implementation

Add a path-only worktree-root function to the existing `agent::repo` owner. Canonicalize cwd and walk its ancestors; at each directory, libgit2 `discover_path` with its parent as ceiling validates that directory's `.git` marker or bare topology without opening repository configuration. The first valid checkout wins; a bare repository stops the search without inventing a checkout. This uses libgit2's gitfile/linked-worktree validation rather than a new marker parser. Existing `RepoDirChain` keeps its home guard and original path spellings.

Root-only Agent, skill, rule, session and inspect callers share this function. Git-directory-only diagnostics/watchers call `discover_path` directly. Trust keeps its root, marker and common-directory filesystem identities: libgit2 validates the Git directory, and the optional `commondir` topology file selects the common directory. Only NotFound means an absent `commondir`; other read errors remain errors. Actual Git operations continue opening repositories and reporting their existing failures.

## Verification

Regression fixtures make `.git/config` invalid/unreadable while retaining valid topology, then exercise root discovery, config/Agent discovery and trust identity. Cover nested roots, linked and separate gitdirs, bare repositories, aliases, malformed gitfiles and changed common-directory identity. Run affected library tests and the normal CLI build. Recheck the real checkout through read-only inspect and isolated session new/load (no inference and no user history writes).
