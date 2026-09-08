# Evidence

- `dispatch_rewind` records the selected shell prompt index; inline edit also uses points loading with a desired target.
- `handle_rewind_points_loaded` resolves that target and directly enters ModeSelect using only `point.has_file_changes`. This bypasses the range-aware picker dispatcher.
- `dispatch_rewind_select_mode` chooses preview versus direct execution based on this flag.
- Picker and back navigation already use `prompt_index >= target`; the shell restores that same checkpoint range.
- Separate async ownership debt is recorded in backlog, without changing result routing in this change.
