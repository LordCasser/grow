
SkillManager baseline reconciliation compares full visible SkillInfo data and order, so same-path metadata changes update AvailableSkills. Identical reloads still skip reconciliation; untriggered conditional skills remain held until activation.

A refreshed skill baseline takes ownership of matching canonical paths, replacing stale dynamic copies before applying conditional gates. Dynamic discoveries outside that baseline remain available with their existing precedence.

Dynamic discovery respects already-loaded skills at the same canonical path: raw file parsing cannot replace configured baseline metadata or bypass a held conditional skill. Baseline reload remains responsible for refreshing known metadata.

Skill substitutions scan only the original template. Argument and context values are inserted literally without recursively expanding token-like text inside them. Missing explicit $ARGUMENTS[N] indices expand to empty; existing positional shorthand and argument suffix rules remain in effect.

Skill internal-link resolution edits only Markdown destination spans. Labels, titles and code remain unchanged; escaped destinations and paths containing spaces are serialized as valid Markdown. Canonical containment checks still restrict rewritten targets to the skill directory.

Skill metadata parsing and body extraction share complete-line frontmatter delimiter detection, matching the streaming reader. Prefixes such as `---suffix` are not delimiters; LF, CRLF and closing delimiters at EOF are supported.

Frontmatter discovery limits underlying file reads to its 4096-byte budget plus one overflow probe byte, including long lines. Truncated UTF-8 at that probe boundary is treated as overflow; invalid UTF-8 within the accepted budget still reports an error. Full skill-body loading is a separate path.

Loaded skill bodies are snapshots: `Some("")` is a loaded empty body and never falls back to disk. Only `None` denotes an unloaded body. Workflow freezing and agent preloading preserve this distinction; synthetic paths without any preloaded body still report an error.

Agent `skills:` declarations respect the selected entry's `enabled` state. A disabled entry is skipped before body loading and does not fall back to a lower-priority skill with the same name; enabled declarations still preload normally.

Skill toggles and `[skills].disabled` use catalog keys: a native skill uses `name`, and a plugin skill uses `plugin:name`. Bare keys affect native skills only, so toggling one entry leaves other identities with the same name unchanged.

`[skills].ignore` path prefixes apply to plugin candidates as well as native skills before merging. Ignoring a plugin directory or individual skill file leaves unrelated paths, including native skills with the same name, available.
