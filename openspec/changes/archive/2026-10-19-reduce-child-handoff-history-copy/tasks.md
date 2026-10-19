## 1. Specification-first change record

- [x] 1.1 Read the suspension contract, archived runtime observation change, and every `CanonicalChildFacts::read` caller.
- [x] 1.2 Record why this is a non-contract ownership refactor.

## 2. Reduce redundant history copying

- [x] 2.1 Add an ignored clone/replay measurement and equivalence probe for 8,000 independent 4 KiB events.
- [x] 2.2 Change the reader to consume owned vectors and scan the validated Timeline; use the already validated cold-load Timeline directly.
- [x] 2.3 Add duplicate-spawn, terminal-child-mismatch, and damaged-Timeline rejection regressions.
- [x] 2.4 Record validation limits and retain the existing deadline and latest-facts reread.
