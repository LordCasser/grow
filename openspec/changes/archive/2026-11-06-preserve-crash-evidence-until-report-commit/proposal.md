# Preserve crash evidence until report commit

## Why

Crash startup recovery ignores report/history publication errors, removes the raw blob and returns a report path that may not exist (PB-07). Simply returning an error is insufficient: CLI currently installs capture immediately afterward, opening the same raw path with O_TRUNC and destroying the preserved evidence.

## What Changes

Recovery reports I/O failure, publishes private reports through atomic file replacement with the existing platform's supported durability barriers, and consumes a valid raw blob only after primary report and history publication succeed. CLI keeps terminal restoration but skips full capture installation when recovery is blocked. Missing or empty raw files remain normal no-crash cases. Multi-process raw identity (PB-05) and cross-launch symbolication (PB-06) remain separate follow-ons.
