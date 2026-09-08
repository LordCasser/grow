# Verification

Red: sampling_auth_logs_omit_credentials failed on the original producer (0/1, 0.00s). Scoped JSON tracing captured complete synthetic bear-secret and api-secret in both client_post header prefix fields and sampling_request auth_prefix. Built request headers were independently checked for the correct values before log assertions.

Source trace: diagnostics/src/sampling_log.rs serializes event/span fields to sampling.jsonl when enabled. client_new already emits presence-only auth metadata. AuthInfo.auth_prefix had only the request-span consumer, so it was removed with that emission. client_post no longer reads header values for logging. Wire header construction and sent_bearer attribution remain unchanged.

The test uses an in-memory scoped subscriber, no global logger/env mutation, no real config/log files and no network. tracing-subscriber was already in the lockfile; this change adds its sampler dev-dependency edge and fmt/json features only. An initial build was stopped to correct the test RequestId qualification before the recorded red run; this was not a product failure.

Green: sampler client::tests:: passed 45/45 in 0.01s, including scoped logging regression and existing header/resolver/attribution tests. Scoped rustfmt and git diff --check passed. Installed binary unchanged.

Strict validation: pre-archive 17/17; post-archive all 16/16 and archives 268/268. No cargo/rustc processes remained before cargo clean, which removed 5,390 files / 1.7 GiB; free disk 56 GiB.
