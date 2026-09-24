## Why

Automatic recap requests and live results currently carry only a session ID. Grow and ACP notifications have no shared ordering, so an automatic recap generated for an earlier away period can arrive after a later away period begins. Pager may display that stale recap and mark the new period satisfied, preventing its own recap.

## What Changes

- Mint an opaque ID when Pager begins each away period and carry it with automatic `grow/recap` requests.
- Echo that ID on the Shell's successful automatic recap notification. Pager displays a live automatic recap only when its ID matches the current away period.
- Keep manual recap feedback and historical replay independent of the automatic away-period gate.

## Capabilities

### Modified Capabilities

- client-surfaces: automatic recap request and notification ownership across focus periods.

## Impact

Pager focus tracking and ACP recap request/result projection; Shell recap command and notification. Model generation and recap content are unchanged.
