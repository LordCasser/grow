# Design
Permission actor initializes remembered state through load_state_from_disk -> load_state_from_dir -> try_load_state. Existing Option distinguishes absent (None) from authoritative state (Some); keep it and return Some(default) for non-NotFound failures. No new type or policy layer needed. Include path in warning. Do not rewrite failed read sources. Existing malformed TOML/schema reset remains.

Use real filesystem temporary directories, shared permissive state, invalid UTF-8 file and directory source. Check no shared grants and unchanged sources. Existing missing-file tests retain shared fallback coverage. Byte budgets, FIFO handling, dangling symlink semantics and read timeouts remain separate.
