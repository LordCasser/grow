# Design: Preserve skill source scope across aliases

Scope order is a precedence and trust boundary: lower enum values override higher ones (`Local < Repo < User`). A config root remains an explicit source selection, so files whose canonical targets remain inside the resolved root retain its scope. A descendant link leaves that source boundary; classify its canonical target relative to known Local, Repo, and User roots, then choose the lower-trust of root and target scopes (the greater enum value). Unknown targets are User. This prevents aliases from promoting content while preserving the existing contract that a local `.grow` root symlink is Local.

Canonicalize both home and user-root identities before home-scope comparison, so equivalent home spellings do not change source classification. Preserve original discovery paths for display and loading. Keep traversal depth, ordering, cycle detection, and deduplication unchanged.
