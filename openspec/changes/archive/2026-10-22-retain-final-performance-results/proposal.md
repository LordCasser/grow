# Proposal

## Why

Recent archived resume-performance changes commit per-run traces, build logs, casts, intermediate samples, and scripts. These artifacts dominate the branch diff while the verified conclusions already live in the final reports.

## What Changes

Remove process artifacts from the affected archived changes. Retain their OpenSpec records, final performance conclusions, measurement conditions, limitations, and validation outcomes. Update stale artifact references and document the retention rule for future performance work.

## Contract Impact

Repository evidence retention changes only. Runtime behavior and archived product contracts do not change; therefore this change skips delta specs.
