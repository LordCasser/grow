# Stop history matching when its owner drops

## Why

History search already coalesces pending requests and makes Drop nonblocking, but the detached worker cannot observe Stop while scoring the active corpus or producing highlights. A closed overlay can therefore retain its history corpus and matcher until the entire request finishes.

## What Changes

Add a shared stop flag checked between per-item scoring and highlight-index operations, and before and after sorting results. Drop sets it before publishing the existing Stop message, so the UI remains nonblocking and the worker abandons the current request at the next check without publishing partial results. A single nucleo score, index operation, sort, or item-building pass is indivisible and has no wall-clock exit deadline.

Scope is limited to `HistorySearchState`'s worker lifecycle. It does not change search ranking, result limits, pending request merging, or the file and scrollback workers.
