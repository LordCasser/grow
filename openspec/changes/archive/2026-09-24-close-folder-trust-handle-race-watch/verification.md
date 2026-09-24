# Verification

- `workspace/src/trust.rs` binds checkout root, `.git` marker and common dir; `shell/src/agent/folder_trust.rs` keys and revalidates cached decisions. `workspace/src/project_config.rs` and `shell/src/config/mod.rs` still perform path-based discovery/read after the verdict. `workspace/src/envrc.rs` sources a path in a child shell; `hooks/src/runner/command.rs` selects command and cwd by path.
- [POSIX `rename()`](https://pubs.opengroup.org/onlinepubs/9799919799/functions/rename.html) requires write access to containing directories, establishing the shared-parent replacement case; [Win32 `CreateProcess`](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw) takes a current-directory path string.
- Recorded the residual race and restart condition in the architecture notes. This no-go does not claim handle-relative safety or eliminate shared-parent attacks.
- `git diff --check -- openspec/backlog.md docs/architecture/behavior-state-overview.md`: passed.
- `openspec validate --all --strict --no-interactive`: 18 passed before archive.
