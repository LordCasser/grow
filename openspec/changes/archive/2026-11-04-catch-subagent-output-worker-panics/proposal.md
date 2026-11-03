# Catch persisted subagent output reader panics

## Why

The coordinator polls persisted-output read futures directly from its owner loop. A panic in one reader currently unwinds that loop, dropping the coordinator and disrupting unrelated child lifecycle and observation requests.

## What changes

Catch unwind only around each persisted-output reader future. If that reader panics, settle every query or inspection attached to that exact deduplicated read as `Unavailable`, release the reader permit through normal future unwinding, and continue serving other coordinator messages.

## Out of scope

Do not catch panics from unrelated coordinator owners, retry the failed read, or change child execution state, durable result state, query authorization, or output-read capacity.
