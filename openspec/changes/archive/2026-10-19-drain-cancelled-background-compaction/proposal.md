# Drain cancelled background compaction owners

## Why

Asynchronous compaction owns a Sideband terminal and Goal usage settlement, so cancellation must keep the original worker reachable until those facts settle. Joining from inside the Goal accounting handler can deadlock with the worker's settlement ACK on the same inbox.

## What Changes

Ordinary authority changes signal cancellation without joining from inside the root inbox; explicit manual-compaction, freeze, and teardown barriers drain the same worker while keeping Goal settlement dispatch available. A freeze fails if its shared deadline expires before that drain is confirmed.
