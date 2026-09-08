## Why
Rewind preview/confirmation transitions retain target_prompt_index in their phase but clear selected_prompt_index. Back navigation reads only selected_prompt_index and falls back to the latest checkpoint, silently changing an older selected target.

## What Changes
Use the current phase's explicit target when returning to mode selection. Keep existing fallback only for phases without a target. Preserve range-based file eligibility and inline-edit exclusion.
