# Hold preview credits through session event consumption

## Why

The sampler channel uses byte credits, but the drainer returns them immediately on receive. It then places the same candidate payload in a second unbounded session-event channel. A busy session actor can therefore accumulate unbounded candidate notifications despite the sampler-channel cap.

## What Changes

Return fragment credits only after the session actor has consumed every event generated from that fragment. Keep control and terminal events outside this payload budget. This closes the second-queue gap without changing preview content or ordering. Gateway and persistence delivery are separate downstream boundaries tracked by the remaining backlog item.
