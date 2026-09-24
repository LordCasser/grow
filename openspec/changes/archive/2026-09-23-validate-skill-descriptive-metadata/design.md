## Context

`parse_skill_frontmatter` feeds all skill discovery paths. It parses YAML values into `serde_yaml::Value`, then `coerce_to_string` converts `String`, `Bool`, and `Number` into text. This is material for `name` (command identity), `description` and `when-to-use` (model-facing skill selection metadata); the same helper also fills optional display fields. Separately, `coerce_tool_list` supports both a delimited string and a list, but `filter_map` silently renders only string elements from a mixed list.

`metadata` already takes only string keys and values, and its `author` projection is used by the skills browser. `allowed-tools` is copied through SkillInfo/RPC and rendered in the extensions modal. A prior consumer audit found no authorization consumer; it is not part of tool access control.

## Decisions

1. **Keep existing fallback semantics, remove invented text.** Change the scalar helper to return text only for YAML strings. A non-string name behaves like an absent name and uses the established directory fallback; a missing or invalid description continues through the existing body preview fallback. Optional trigger, license, compatibility, and argument-hint values become absent. This keeps malformed scalar types from changing identity, model routing text, or display values without turning harmless display typos into a whole-skill failure.
2. **Do not publish partial tool declarations.** Keep both currently supported `allowed-tools` forms, but accept a YAML sequence only when every item is a string. A wrong top-level type or any non-string list element yields no `allowed_tools` value. This has no authorization effect.
3. **Do not widen metadata policy.** `metadata` remains a string-key/string-value projection; non-string entries are ignored as before. Invocation switches and `paths` retain their already-strict error behavior. The aliases and other valid field semantics remain unchanged.

## Risks / Trade-offs

- A malformed numeric/boolean value that previously appeared as text will now use a documented fallback or disappear from metadata. This is the intended correction; valid quoted strings are unchanged.
- Invalid mixed `allowed-tools` lists stop showing their valid subset. Showing a partial declaration falsely suggests that Grow accepted the complete list.
- `allowed-tools` remains non-enforcing metadata; no permission guarantee is introduced.
