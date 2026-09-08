# Sequential deletion verification

Prior fixes committed first as 175b0836, retaining all candidate implementations. Baseline: tools/shell/pager/workspace-types library test targets compiled; tools implementations::skills:: 104/104 passed. Two whitespace findings in previously untracked historical markdown were observed during staged validation; fix separately before release, not by rewriting the baseline commit.

R1 retained by user instruction. R7 retained because external initialization consumers cannot be exhaustively excluded; no persistence code changed.

## R2
Cargo metadata --locked --offline --no-deps resolved successfully. Compared declared features with committed pre-deletion manifest: exactly unstable removed, all other feature declarations unchanged. The feature had no runtime implementation, so this manifest regression is the relevant verification.

## R3
Removed unused resolver, its sole module/exports and RemoteSettings field. Repository source search finds no remaining symbol/env consumer. config-types remote_settings: 27/27 passed; git diff --check passed.

## R4
Removed only unused internal ID formatter selector/branch and seven exclusive formatter tests. Freshly rebuilt tools ask_user_question group passed 59/59; normal notes, IDs and duplicate-label validation remain covered. An earlier cached run was discarded after restoring file timestamps; only the rebuilt 59-test run is counted.

## R5
SkillManager owns active listings/slash advertisement; duplicate AvailableSkills type and writes removed. Migrated startup+dynamic preservation assertions to SkillManager. Fresh tools skill-filter regressions passed 200/200, including manager, discovery and registry coverage.
