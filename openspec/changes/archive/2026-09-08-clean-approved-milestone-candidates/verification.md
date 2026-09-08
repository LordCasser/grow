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

## R6
Removed unregistered SkillInput/SkillOutput and exclusive protocol/normalization/permission/ACP/preparation branches. Retained skill prompt loading and ToolKind::Skill. Fresh tools skill regressions 200/200 and shell acp_conversion 29/29 passed; no remaining old IO symbol references. Existing nonfatal linker unwind warning only.

## R8
Removed unused permission policy/effect/result helpers and exclusive tests/fixtures. Shared PersistSetting, NotifySessionPermissionMode, rollback and queue behavior retained. Fresh pager modes regressions passed 37/37 after final fixture cleanup; regular AllowOnce negative permission-change regression passed 1/1. Old protocol symbols absent; only existing nonfatal linker warning.

## R9
Removed only two uncalled private YAML repair helpers and RECOVERABLE_KEYS. Fresh tools discovery regressions passed 41/41, preserving strict metadata rejection and valid Markdown fallback. No remaining helper symbols; git diff --check passed.

## R10
Removed unused skill model/effort fields from discovery, both SkillInfo representations and mirrored test fixtures; session sampling fields unchanged. Initial compilation exposed remaining test-only literals, which were removed before final validation. Fresh tools skill 200/200, agent prompt::skills 94/94 and workspace-types rpc::skills 7/7 passed; git diff --check passed.

### R10 integration follow-up
Shell test-target compilation found three remaining SkillInfo fixture literals. Removed only their obsolete model/effort fields. Rebuilt shell slash_commands regressions passed 98/98 before resuming R11; no production behavior changed.

## R11
Removed the ignored SkillSlashRewrite selector and all production/test arguments. Fresh shell session::slash_commands regression passed 98/98; expansion and out-of-band rejection assertions retained. No selector references remain.

### R10 Pager integration follow-up
Pager test-target compilation found one remaining extension-modal SkillInfo fixture. Removed its obsolete fields; fresh views::extensions_modal regressions passed 142/142. A whole-crates scan found no remaining SkillInfo literal containing model/effort. The entire Pager library test target now compiles.

## Final integration
Nine approved candidates removed in separate commits; R1 and R7 retained. Candidate statuses and developer links reconciled. Two preexisting historical Markdown whitespace findings normalized. Strict all validation passed before archive; targeted regression counts above are actual runs, not claims of full-suite coverage.
Post-archive validation: current specs/active changes 17/17 (including release preparation); archived changes 272/272 passed.
