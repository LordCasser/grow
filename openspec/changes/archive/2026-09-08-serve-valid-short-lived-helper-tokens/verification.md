# Verification

Red: short_lived_token_is_sendable_until_expiry_or_failed_refresh failed on the old resolver: None instead of Some("tok-1") immediately after mint (0/1, 0.05s).

The regression uses the production bearer_resolver plus synthetic local helper output. It covers a 30-second configured lifetime, a token-shaping config edit, actual expiry via the existing test hook, and a subsequent near-expiry refresh failure by removing the temporary working directory. No process-global cwd/environment changes or live provider calls.

Source trace: agent/config.rs::resolve_credentials consumes cached_token; session pre-turn refresh calls ensure_fresh_token; sampler/client.rs::post removes default authentication headers and exclusively uses current_bearer when a resolver exists. This test exercises the actual resolver boundary, not a live HTTP request.

Green: shell auth:: passed 35/35 in 1.05s, including existing expiry precedence, 401 guards, failed refresh, config invalidation and process bounds. Scoped rustfmt and git diff --check passed. Existing nonfatal linker __eh_frame warning remained. Installed binary unchanged.

Pre-archive strict validation passed 17/17. The remaining external cargo process was verified by cwd as ScriptOS, a separate project; this Grow test session was terminal before cleaning its target.
Post-archive strict validation: all 16/16, archives 267/267. cargo clean removed 7,372 files / 2.7 GiB; free disk 56 GiB.
