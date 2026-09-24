## Why

Two Shell leader/server tests assume that the next ACP-channel message after a client disconnect is the only relevant notification. The server also emits `grow/internal/queue_client_disconnected` for every disconnected client, so test scheduling can expose that housekeeping message before the eviction assertion or while checking driver transfer.

## What Changes

Update only the test receive/assertion logic to consume the expected disconnect notification and continue checking the intended behavior: disconnected sessions are evicted when no subscriber remains, while a remaining subscriber retains the session and receives driver-routed requests.

This is test maintenance only. Server behavior and OpenSpec behavior contracts do not change.
