# Evidence
- ACP initialize schedules cleanup_stale_sessions(None) on spawn_blocking; load also schedules it with current session skip. A process Once allows only the first invocation, so this is an active startup cleanup path.
- resolve_cleanup_ttl_days accepts a positive TOML i64 and returns days as u32. 4294967296 therefore becomes zero; other large positives become unrelated smaller TTLs. This arithmetic is source-derived, not an actual cleanup run.
- cleanup_stale_sessions_sync subtracts chrono::Duration::days(u32 days) directly from Utc::now. Representable u32 day counts can exceed DateTime's range; checked cutoff handling is absent. Needs focused boundary test, not a claim of observed production panic.
- scan_opened_sessions captures summaries. Cleanup checks last_active_at (or updated_at), then acquires the writer lease and calls delete_opened_session without rereading activity. Another writer can refresh activity and release its lease between scan and acquisition.
- delete_opened_session verifies entity identity through quarantine rename and retains writer exclusivity; this protects against entity substitution, but does not refresh activity eligibility of the same entity.
- Existing leases protect currently held writers; they do not prove scanned timestamps remain current after a lease is acquired. No live session or user data was deleted or modified in this audit.

# Separate repairs
1. Reject invalid/out-of-range TTL configuration without truncation and compute cutoff safely; failed eligibility setup must not delete history.
2. Refresh identity-validated activity under the acquired writer lease before removing a stale candidate. Preserve skip and active-writer protection.
