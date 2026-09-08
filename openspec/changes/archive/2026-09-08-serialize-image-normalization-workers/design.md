# Evidence and design
Dashboard mixed drop/URL fallbacks were already repaired; current header-only preview validation does not decode pixels. Kitty conversion has a separate 16M pixel limit, while shell normalization deliberately accepts up to178956970 pixels to support camera images before downscale. Both transcode and normal compute use normalize_cache::run_blocking. Its old adapter has no semaphore; cache coalescing is optional and only per content key.

Use static tokio Semaphore::const_new(1), acquire before spawning and retain borrowed static permit inside closure until completion/panic. No queue actor, no changed image quality/size acceptance. Waiting closures may retain encoded data; this is a decode concurrency bound, not total process memory or queue byte bound. Test cancellation while worker is blocked and ensure next worker cannot enter until actual completion. Existing panic mapping must still work.

Separate boundaries: arboard system allocation and PNG encoding, direct PNG/iTerm terminal-side decoding, pending encoded payload memory remain unaudited resource budgets.
