# Verification

- Before maintenance, the resident-reload assertion rejected a valid transient `inquiry approval` containing the current approved nonterminal audit. This exactly matches the source's latest-Timeline-fact fold and approval-before-inference ordering.
- `python3 scripts/test_local_coordination.py --binary target/debug/grow` passed all ten scenario groups with the corrected assertion. Log: `/tmp/grow-v2.1.6-coordination-final.log`.
- Resident reload now checks one transient snapshot, exact current approval subject, source session, original question, approved same-workspace decision and null outcome. The existing no-duplicate-model-request check and every subsequent crash/restart scenario remain present and passed.
- Native execution used temporary GROW_HOME and workspaces plus --no-leader. Coordination peer discovery is rooted in that isolated GROW_HOME; no existing user process was stopped or replaced.
- `git diff --check` passed. Cross-platform execution is additionally covered by the existing GitHub workflow before release.
- Post-archive OpenSpec validation passed: 17 current items and 308 archived changes.
