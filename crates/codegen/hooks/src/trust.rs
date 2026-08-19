use std::path::{Path, PathBuf};

// ── Hook enable/disable ─────────────────────────────────────────────────

/// Check whether a hook is disabled by name.
///
/// Disabled hooks are listed in , one hook name per line.
pub fn is_hook_disabled(hook_name: &str) -> bool {
    match disabled_hooks_file_path() {
        Some(file) => is_hook_disabled_with_file(hook_name, &file),
        None => false,
    }
}

fn is_hook_disabled_with_file(hook_name: &str, file: &Path) -> bool {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return false,
    };
    content
        .lines()
        .any(|l| !l.trim().is_empty() && !l.trim().starts_with('#') && l.trim() == hook_name)
}

/// Disable a hook by name. Adds to .
pub fn disable_hook(hook_name: &str) -> Result<(), String> {
    let file = disabled_hooks_file_path()
        .ok_or_else(|| "no user grow home (set $GROW_HOME or $HOME)".to_string())?;
    disable_hook_with_file(hook_name, &file)
}

fn disable_hook_with_file(hook_name: &str, file: &Path) -> Result<(), String> {
    if is_hook_disabled_with_file(hook_name, file) {
        return Ok(()); // Already disabled.
    }
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .map_err(|e| format!("failed to open disabled-hooks file: {e}"))?;
    writeln!(f, "{hook_name}").map_err(|e| format!("failed to write disabled-hooks file: {e}"))?;
    Ok(())
}

/// Enable a hook by name (remove from ).
pub fn enable_hook(hook_name: &str) -> Result<bool, String> {
    match disabled_hooks_file_path() {
        Some(file) => enable_hook_with_file(hook_name, &file),
        None => Ok(false),
    }
}

fn enable_hook_with_file(hook_name: &str, file: &Path) -> Result<bool, String> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("failed to read disabled-hooks file: {e}")),
    };
    let mut found = false;
    let new_lines: Vec<&str> = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') && trimmed == hook_name {
                found = true;
                false
            } else {
                true
            }
        })
        .collect();
    if !found {
        return Ok(false);
    }
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    use std::io::Write;
    let mut f = std::fs::File::create(file)
        .map_err(|e| format!("failed to open disabled-hooks file: {e}"))?;
    for line in new_lines {
        writeln!(f, "{line}").map_err(|e| format!("failed to write disabled-hooks file: {e}"))?;
    }
    Ok(true)
}

/// Returns the path to `$GROW_HOME/disabled-hooks`, or `None` when no user grow
/// home resolves.
fn disabled_hooks_file_path() -> Option<PathBuf> {
    Some(config::user_grow_home()?.join("disabled-hooks"))
}
