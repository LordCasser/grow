## Why
Goal excludes cache-hit input from its persisted charge, so both its Usage display and budget differ from provider total tokens.

## What Changes
Persist and display cache-hit input, cache-miss input, output and their total. All Goal budgets use full input plus output, including cached input; reasoning is already included in output. Carry categorized usage through existing exactly-once settlement and atomic persistence. Historical aggregate-only usage remains an explicit lower bound with unavailable categories; it cannot enforce an exact budget. Unbudgeted Goals continue normally.
