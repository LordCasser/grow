Execution is tracked per item in tasks.md and verification.md; the following audit describes the approved removal boundaries, not completion of future commits.

# Per-candidate review

R1: explicitly retained; no lock-manager edits.
R2: repository Cargo/script/source search found no shell/unstable enablement or cfg; only the empty declaration removed. Other dependencies' unstable-named features retained.
R3: no consumers of resolver or field; removed the only contents of resolve/features.rs and its module exports. No data-retention policy inference.
R4: wire/schema skip prevents activation and only tests set true. Removed alternate formatter and exclusive tests; duplicate-label validation and normal notes tests retained.
R5: resource getters were test-only; bridge.slash_skills and listings use SkillManager. Removed duplicate writes/type; migrated registry startup+dynamic preservation test to manager.slash_skills. take_pending projections and effects preserved.
R6: no registered Skill Tool implementation or producer; ToolPack remains dynamic. Removed legacy Input/Output variants and exclusive normalization/authorization/ACP/display branches. Prompt loading, path/argument substitution and ToolKind::Skill retained. No backward wire compatibility layer added.
R7: whole-repository save_config search finds only definition, save_config_locked call, and documentation. Known grow-fork GitHub code search returned no matches, which cannot rule out external clients or initialization scripts. Retained the public wrappers, update_config, save_config_at and atomic storage entirely. No machine initialization behavior changed.
R8: production setters use PersistSetting and NotifySessionPermissionMode. Old policy/helper/results only executor and old tests. Removed those; migrated negative permission-option assertions to active effects, retained default-preference/rollback/queue tests. No real config-writing legacy tests executed.
R9: private repair helpers and constant had zero callers. Removed only that region; strict YAML and Markdown fallback behavior retained.
R10: metadata parsing/copying/RPC serialization only, no sampling consumer. Removed fields in both mirrored SkillInfo types, frontmatter parsing/copy/defaults and exclusive assertions. Kept allowed_tools/enablement/invocation restrictions and session model/effort.
R11: resolve ignored parameter; production always supplied RewriteToRun. Removed enum, parameter and call arguments, including test wrapper arguments. Existing skill expansion and out-of-band rejection tests retained.

External public-crate consumers are not exhaustively knowable. R7 is deliberately retained because the user's instruction singles out that uncertainty. R12–R32 are not authorized by this request and remain unchanged.
