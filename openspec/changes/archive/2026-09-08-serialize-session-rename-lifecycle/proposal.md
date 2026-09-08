## Why
Session rename bypasses the lifecycle guard used by load and delete, allowing its dormant/live decision and title write to race those operations.

## What Changes
Serialize rename lookup and commit under the existing per-session lifecycle guard, avoiding waits on a loader queued behind that guard.
