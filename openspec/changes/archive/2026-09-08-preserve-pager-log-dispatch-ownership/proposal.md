## Why
Pager log batches are drained before sender/runtime availability is assured. Logging on a plain thread can silently lose a batch, and a flush before initialization consumes startup logs. Repeated init also creates extra periodic consumers.

## What Changes
Capture sender and dispatch runtime together at successful initialization, preserve buffered entries when uninitialized, and start exactly one periodic consumer. Keep transport and wire format unchanged. Independently track shutdown acknowledgement and overload budgets.
