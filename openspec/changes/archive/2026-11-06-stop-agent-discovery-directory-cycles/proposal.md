# Stop Agent discovery directory cycles

## Why

Recursive Agent Markdown discovery follows directory links without tracking the physical ancestor chain (AC-05). A link to an ancestor repeats the same directory under growing lexical namespaces until the OS link limit, instead of terminating at the cycle. This affects advertised/discovered Agent definitions.

## What Changes

Track canonical directory identities only on the current recursion stack and skip a link that revisits an ancestor. Preserve lexical path-derived Agent IDs, stable order, and legitimate non-cyclic directory aliases. Skip directories whose identity cannot be resolved rather than recursing with an unproven lexical identity. Do not add a global visited registry, arbitrary namespace depth cap or change provider/session behavior.
