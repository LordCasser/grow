# Evidence

RewindSelectMode and preview completion preserve target_prompt_index in Previewing/Confirm/ConversationOnlyConfirm but clear selected_prompt_index. Back navigation previously ignored the phase target and resolved a missing selection to the latest checkpoint.

Red rewind_back_from_confirmation_preserves_older_target: 0 passed / 1 failed, 0.06s. The actual dispatcher preview/confirmation flow selected target 2 but back navigation returned target 4. The red test stopped on the first case; missing metadata and target-zero confirmation are checked on the fixed implementation.

Back navigation now reads explicit targets from target-bearing phases, using the previous fallback only when no phase target exists. ModeSelect stores the resolved target as its selection and reuses the existing range-eligibility calculation. No preview execution or backend file mutation code changed.
