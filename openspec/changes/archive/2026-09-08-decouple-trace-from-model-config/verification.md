# Verification

Red: trace_reaches_storage_with_invalid_model_config failed against the original CLI Trace arm (0/1, 0.07s). Child reached the actual async_main dispatch and returned Failed to load config: TOML parse error instead of the missing-session error. The malformed fixture was explicitly rejected by the loader before dispatch.

Test runs in a fresh exact-filter subprocess with temporary HOME, GROW_HOME and cwd. It does not export any real session. The child scrubs optional external log/sandbox environment controls; none were present during the red run. The main process environment is not mutated. Uses current-thread Tokio runtime only inside the child.

Production change removes only the unused required effective-config load and AgentConfig construction from Trace. Common startup best-effort reads, session storage lookup and archive writing remain unchanged. This regression proves dispatch reaches missing-session validation with malformed config; it is not a valid-session archive-content or filesystem-failure test, nor a full main/bootstrap test. Installed binary not updated.

Green: isolated CLI regression passed 1/1 in 0.01s. Scoped rustfmt and git diff --check passed. The existing nonfatal linker __eh_frame warning remained. tempfile was added as a CLI dev-dependency using the existing locked version, without a dependency version upgrade.

Strict validation passed: pre-archive 17/17; post-archive all 16/16, archives 271/271. No cargo/rustc process remained before cargo clean, which removed 9,055 files / 3.6 GiB; free disk 56 GiB.
