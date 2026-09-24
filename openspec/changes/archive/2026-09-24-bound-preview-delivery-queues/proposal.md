# Bound preview delivery after the session actor

## Why

Candidate bodies no longer enter persistence, and sampler/session event handoffs are byte-budgeted. The live gateway still accepts candidate payloads into an unbounded queue without waiting for the client. Independent untagged notifications can likewise accumulate in the unbounded persistence sender while an attempt is active. A slow downstream consumer can therefore make an attempt's memory use grow without a bound.

## What Changes

Reserve a per-session byte budget before enqueuing live preview notifications into the gateway, releasing each reservation only when its gateway request completes. If the budget cannot admit a notification or a single notification exceeds it, mark the attempt's preview delivery failed and reject canonical admission. During an active attempt, await persistence acknowledgement for each independent untagged ACP notification so the actor cannot build an unbounded persistence queue. Retain the existing payload-free candidate anchor and canonical admission barrier.
