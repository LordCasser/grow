## Why

`ShowContextInfo` currently carries the target session only into the worker;
its `TaskResult` carries neither session identity nor binding epoch. A late
result can therefore update live context state or a reopened modal after the
AgentView has been rebound.

## What Changes

Bind successful and failed results to the request's session and view binding
epoch before any live or modal projection. A nonzero modal nonce must also
match before any mutation, including when only the modal is closed and reopened.
Retain the zero-nonce scrollback route and add focused regression coverage.

## Capabilities

Update client-surfaces with the asynchronous context result ownership contract.

## Impact

Pager context request effects, task results, reducers and tests; no backend
protocol or durable storage format changes.
