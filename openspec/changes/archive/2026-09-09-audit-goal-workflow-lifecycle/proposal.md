## Why
Repeated Goal and Workflow regressions warrant a connected lifecycle audit rather than isolated symptom fixes. The user specifically requires unlimited Goal token consumption unless an explicit token budget is supplied.

## What Changes
Audit existing implementation, callers, persisted facts and regression coverage. Record confirmed issues and fix each through its own behavior change. This parent change records audit evidence only and does not change product contracts. Unrelated architectural debt stays separate.
