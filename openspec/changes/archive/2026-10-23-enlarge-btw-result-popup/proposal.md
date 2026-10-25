## Why

The fullscreen TUI currently puts `/btw` answers in a short inline strip above the prompt. Long answers wrap and scroll in a cramped area. After dismissal, double-clicking the saved `/btw` row only folds it inside the transcript.

## What Changes

- Show the active `/btw` question and answer in a large popup over the transcript, sized like the fullscreen Plan preview while leaving the prompt available.
- Open a saved `/btw` answer in a popup of the same size on double-click or the existing viewer action.
- Keep the Sideband request, Esc dismissal, transcript persistence, and minimal terminal mode behavior intact.

## Capabilities

- `client-surfaces`: fullscreen TUI `/btw` presentation and revisit gesture.
