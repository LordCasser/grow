# Test isolation incident

The first pager effect test changed GROW_HOME after config::grow_home OnceLock had already initialized. The test returned success but the expected temporary file was missing. Inspection confirmed the default ~/.grow/announcements.json contained exactly the test marker {"hidden_ids":["notice"]}, modified at 2026-09-08T06:52:40 local filesystem time. This was an unintended write to real user state.

The marker was verified and removed; no prior contents were captured, so previous hidden preferences cannot be proven restored. No other user file was inspected or modified for this investigation. Subsequent verification must use a fresh subprocess with GROW_HOME set before startup and assert the resolved path before invoking the production effect. The earlier claim of no real user configuration writes was incorrect for the failed run.
