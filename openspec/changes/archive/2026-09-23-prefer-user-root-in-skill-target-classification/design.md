# Design: Prefer User roots when classifying linked skill targets

`scope_for_discovered_skill` first checks whether a file remains inside its selected source root; that preserves explicit Local root aliases. If the file leaves the source root, target classification must prefer the canonical User root before an overlapping cwd `.grow` alias. Repo roots are then checked, with unknown paths defaulting to User. The root-scope cap continues to choose the lower-trust scope.
