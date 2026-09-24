# Change: Stop session actors when their agent owner ends

## Why

An in-process leader generation can drop `MvpAgent` while its dedicated session actor OS threads remain active. A replacement generation then cannot acquire the same session's writer lease, so the reconnect scenario fails at `session/load`. A real killed leader process ends all its threads; the in-process lifecycle must explicitly retire actors when the agent owner ends.

## What Changes

- Ask every resident primary and active child session actor to shut down when `MvpAgent` drops, before dropping the handles.
- Make the in-process leader fixture wait for the old writer lease to be released before starting a replacement generation, rather than retrying a racing load or sleeping a fixed duration.
- Run the exact-test reconnect scenario through history restoration and a new turn.

## Impact

Shell agent teardown and Pager leader test fixture. No persistent format or external protocol changes.
