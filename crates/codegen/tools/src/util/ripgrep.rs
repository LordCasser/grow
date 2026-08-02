use std::path::PathBuf;
use std::sync::OnceLock;

#[cfg(bundle_rg)]
const RG_BYTES: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/bundle-rg/rg-",
    env!("GROW_TOOLS_RG_VER"),
    "-",
    env!("GROW_TOOLS_RG_TARGET"),
    ".bin"
));

#[cfg(bundle_rg)]
fn resolve_bundled_rg() -> std::io::Result<PathBuf> {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    let binary_name = concat!(
        "rg-",
        env!("GROW_TOOLS_RG_VER"),
        "-",
        env!("GROW_TOOLS_RG_TARGET")
    );
    let binary_name = if cfg!(windows) {
        format!("{binary_name}.exe")
    } else {
        binary_name.to_string()
    };
    let path = crate::util::grow_home().join("vendor").join(binary_name);
    if !path.exists() {
        fs::create_dir_all(path.parent().expect("vendor path has a parent"))?;
        fs::write(&path, RG_BYTES)?;
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&path)?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions)?;
        }
    }
    Ok(path)
}

/// Resolve the canonical ripgrep executable used by every Grow search path.
///
/// GitHub Release builds extract their embedded binary into
/// `~/.grow/vendor`. Source builds use `RG_BIN_PATH` when set, then fall back
/// to `rg` on PATH.
pub fn rg_path() -> PathBuf {
    static RG_EXEC: OnceLock<PathBuf> = OnceLock::new();
    RG_EXEC
        .get_or_init(|| {
            #[cfg(bundle_rg)]
            {
                resolve_bundled_rg().unwrap_or_else(|_| PathBuf::from("rg"))
            }
            #[cfg(not(bundle_rg))]
            {
                if let Ok(path) = std::env::var("RG_BIN_PATH") {
                    return PathBuf::from(path);
                }
                if let Ok(runfiles_dir) = std::env::var("RUNFILES_DIR") {
                    let base = PathBuf::from(runfiles_dir);
                    if let Ok(entries) = std::fs::read_dir(&base) {
                        for entry in entries.flatten() {
                            let name = entry.file_name();
                            if name.to_string_lossy().contains("ripgrep_hermetic") {
                                for child in ["amd64/rg", "arm64/rg", "rg"] {
                                    let candidate = entry.path().join(child);
                                    if candidate.exists() {
                                        return candidate;
                                    }
                                }
                            }
                        }
                    }
                }
                PathBuf::from("rg")
            }
        })
        .clone()
}

#[cfg(test)]
mod tests {
    #[cfg(bundle_rg)]
    use super::*;

    #[cfg(bundle_rg)]
    #[test]
    fn bundled_rg_is_runnable_without_path_lookup() {
        let path = rg_path();
        assert_ne!(path, PathBuf::from("rg"));

        let output = std::process::Command::new(&path)
            .arg("--version")
            .env_clear()
            .output()
            .expect("run embedded ripgrep by its extracted path");
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).starts_with("ripgrep "));
    }
}
