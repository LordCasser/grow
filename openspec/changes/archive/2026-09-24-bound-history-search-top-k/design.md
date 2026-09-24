# Design

Keep the existing full item corpus because matching and query updates use it, but bound temporary scored candidates to `MAX_RESULTS`. A max-heap stores the worst retained hit at its root (lower score, then later source index); each scored hit replaces that root only when it ranks better. Sorting is then limited to at most 100 entries. Equal scores retain earlier source entries, and the existing reverse-before-publication behavior keeps the best score at the bottom of the overlay.

Make `build_items` return `None` when the shared stop flag is set, checking before allocation and around each per-entry conversion. The worker installs converted items and begins matching only after preprocessing completes. The existing stop flag and pending Stop message remain the cancellation mechanism; one score, UTF-32 conversion, or highlight-index operation remains indivisible.
