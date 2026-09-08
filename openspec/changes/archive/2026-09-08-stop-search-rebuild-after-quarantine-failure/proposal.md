## Why
Search-cache quarantine currently logs sidecar/main rename failures but still calls recreate. Its Option<PathBuf> result conflates an absent main file with failure to isolate it, and a successful main rename followed by failed recreation still emits a recreated-empty-cache warning. Rebuilding at a path whose isolation failed is not a verified recovery boundary.

## What Changes
Propagate quarantine failures and stop that recovery attempt before recreate. Distinguish absent files from rename failures. Report actual quarantine/recreation outcomes without claiming success when recreation failed. Preserve existing confirmed-corruption reprobe and canonical session data.
