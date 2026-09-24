## Why

In the instrumented 512-turn PTY resume, Pager paints 253 frames during history replay, spending about 4.36 seconds in rendering while the same-process Shell load stretches to about 9.11 seconds. The actual terminal writer takes only about 9 ms. Automatic replay and animation paints are competing with ACP processing on the UI loop.

## What Changes

- Apply a 100 ms minimum interval to automatic ACP, animation, and periodic UI-maintenance paints while the visible agent is loading historical replay; retain the user's configured slower interval.
- Keep user input, resize, and other explicitly requested redraws immediate; return to the normal cadence when the load completes.
- Recheck long-session PTY history visibility, frame count, and key echo, plus existing input-during-replay behavior.

## Capabilities

### Modified Capabilities

- `client-surfaces`: cold session replay limits automatic paint work while preserving progress, input responsiveness, and the session-loaded barrier.

## Impact

Pager event-loop paint scheduling only; no changes to ACP replay, model context, persistence, or terminal writer ordering.
