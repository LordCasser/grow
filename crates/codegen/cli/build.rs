use std::process::Command;

#[path = "../version/git_identity.rs"]
mod git_identity;

fn main() {
    let manifest_dir = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"),
    );
    for path in git_identity::watch_paths(&manifest_dir) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!("cargo:rerun-if-env-changed=GROW_VERSION");
    println!("cargo:rerun-if-env-changed=RELEASE_COMMIT");

    let commit = match std::env::var_os("RELEASE_COMMIT") {
        Some(value) => {
            let value = value.to_str().expect("RELEASE_COMMIT must be Unicode");
            assert!(
                value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "RELEASE_COMMIT must contain exactly 40 ASCII hexadecimal characters"
            );
            value[..7].to_ascii_lowercase()
        }
        None => Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string()),
    };

    let version = std::env::var("GROW_VERSION")
        .or_else(|_| std::env::var("CARGO_PKG_VERSION"))
        .unwrap_or_else(|_| "0.0.0".to_string());

    println!(
        "cargo:rustc-env=VERSION_WITH_COMMIT={} ({})",
        version, commit
    );
}
