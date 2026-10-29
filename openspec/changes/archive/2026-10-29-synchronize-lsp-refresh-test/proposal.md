# Synchronize the LSP refresh regression

## Why

`a_refresh_we_cannot_act_on_does_not_discard_what_we_know` waits a fixed 200 ms after seeing the pushed diagnostic. That delay does not prove the mock server has received the response to its refresh request, so a slow or differently scheduled test process can inspect state too early.

## What Changes

- Have the mock expose an acknowledgement only after it receives the response to `workspace/diagnostic/refresh`.
- Wait for that protocol event before asserting that the previously published diagnostic remains present.

## Scope

This is test synchronization only. It does not change runtime behavior or the LSP contract, so no delta specification is added.
