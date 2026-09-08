# Design

The phase already owns the active operation target. Read that field from target-bearing phases before taking rewind_state; use it ahead of the old list/selection fallback. Do not duplicate target state across every transition. Re-enter ModeSelect with selected_prompt_index set to the resolved target. A missing/refreshed point list must not retarget an existing confirmation.

Tests traverse actual dispatcher actions through RewindSelectMode and RewindPreviewComplete, then BackToModeSelect. Cover an older target with newer checkpoints, missing cached metadata on return, and ConversationOnly target-zero confirmation. Existing tests that called back directly from ModeSelect did not catch the reset after preview.
