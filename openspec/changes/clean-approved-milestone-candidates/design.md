## Boundaries
R1 lock manager retained. R2 empty feature and R3 unused ZDR resolver/field removed. R4 alternate question formatting only; retain IDs/notes/normal output. R5 resource snapshot only if SkillManager owns real reads. R6 legacy IO only; preserve skill loading. R7 preserve until external and initialization paths investigated. R8 old persistence protocol only; preserve current setting coordinator. R9 uncalled YAML repair helpers. R10 skill model/effort metadata only, no session sampling changes. R11 unused rewrite parameter only.

## Validation
Recheck symbols before/after edits; run relevant remaining skill/question/settings/config/slash tests and compile affected crates. No user state mutation in tests. Record scope and unresolved external evidence honestly.
