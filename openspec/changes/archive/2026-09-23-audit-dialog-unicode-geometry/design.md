# Design: Audit dialog and viewport Unicode geometry

Trace input display rendering from production callers through viewport byte ranges and cursor display columns. Inspect width calculations for dialog labels, search labels, expanded picker fields, modal tabs, and headers; determine whether any byte-counted string can contain non-ASCII text in production. Use existing tests as evidence and preserve unrelated geometry backlog clauses.
