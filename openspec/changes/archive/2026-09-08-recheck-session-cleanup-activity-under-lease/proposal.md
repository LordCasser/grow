## Why
Session cleanup decides age from a snapshot taken before writer lease acquisition, permitting deletion based on stale activity after another writer has completed.

## What Changes
After acquiring the existing writer lease, reread and validate the candidate summary through its pinned directory and re-evaluate activity cutoff before quarantine/delete. Do not use a cached summary accidentally. Preserve identity checks, explicitly skipped session, current writers and future activity. Add deterministic fixture that refreshes a candidate between initial scan and lease-time eligibility; no actual user history cleanup.
