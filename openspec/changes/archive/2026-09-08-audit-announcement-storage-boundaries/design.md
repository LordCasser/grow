# Actual entrypoints
shell acp_agent.rs initialization schedules emit_announcements after two seconds. agent_ops.rs filters expiry and publishes a replacement snapshot. ConfigUpdate::Announcements injects the internal reload request. Pager also seeds local configuration at startup, consumes announcements/update, prunes hidden IDs and updates slash visibility. The AcpConnection startup_announcements comment concerns an unused InitializeResponse.meta transport proposal; it is not evidence that announcements are unimplemented.

Shared announcements::default_announcements supplies an info message unless local configuration explicitly supplies an empty array. Pager views select critical/promo session banners and other welcome presentation using severity, expiry, message and dismissibility. Existing tests cover snapshot replacement, pruning, banner selection and action emission; inspected tests are not fresh runtime verification.

# Storage findings
1. read_hidden_announcement_ids awaits tokio::fs::read_to_string during event_loop startup. There is no byte limit or ordinary-file check. Malformed/missing files fail open, but oversized files allocate without a budget and special targets can occupy blocking IO and delay startup. This is mechanism evidence, no real user-file reproduction.
2. write_hidden_announcement_ids calls tokio::fs::write and discards its Result. Effect::PersistAnnouncementsHidden always returns TaskResult::AnnouncementsHiddenPersisted{result:Ok(())}, leaving the existing warning branch unreachable for actual write failures.
3. Hide/show/update-prune generate whole-set snapshots and each effect spawns independently. Truncating writes can overlap; even atomic replacement alone would not establish user-action order. Treat safe file commit and ordering as distinct repair boundaries.
4. No explicit parent creation or atomic commit in this storage helper. Parent availability is normally incidental to GROW_HOME initialization, not guaranteed by the helper.

# Unused field
Announcement.persistent is declared, defaulted to Some(false) and serialized, but the shared filter and pager announcement/welcome render/control paths do not consume it. Candidate R24 requests confirmation before removal; serde transport and external consumer use remain unknown. Do not conflate it with dismissible or hidden-ID persistence.

# Scope
No user logs/config files read, no runtime code changed, no test build or cargo artifacts created. Next bounded repair is atomic hidden-state publication with truthful error propagation; read limits and per-process snapshot ordering are separately recorded.
