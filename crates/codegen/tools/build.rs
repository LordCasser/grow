//! Build script for bundling search tools for the tools crate.
//!
//! Release packaging supplies prebuilt binaries explicitly. Ordinary source
//! builds never access the network and fall back to tools available on PATH.
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const RG_VER: &str = "15.0.0";
const BFS_VER: &str = "4.1";
const UGREP_VER: &str = "7.7.0";
const FD_VER: &str = "10.4.2";
// fd stopped publishing x86_64-apple-darwin assets after 10.3.0.
const FD_VER_MACOS_X64: &str = "10.3.0";

/// Pinned SHA-256 of each `(version, triple)` fd release tarball we embed.
const FD_TARBALL_SHA256: &[(&str, &str, &str)] = &[
    (
        "10.4.2",
        "x86_64-unknown-linux-musl",
        "e3257d48e29a6be965187dbd24ce9af564e0fe67b3e73c9bdcd180f4ec11bdde",
    ),
    (
        "10.4.2",
        "aarch64-unknown-linux-musl",
        "f32d3657473fba74e2600babc8db0b93420d51169223b7e8143b2ed55d8fd9e8",
    ),
    (
        "10.4.2",
        "aarch64-apple-darwin",
        "623dc0afc81b92e4d4606b380d7bc91916ba7b97814263e554d50923a39e480a",
    ),
    (
        "10.3.0",
        "x86_64-apple-darwin",
        "50d30f13fe3d5914b14c4fff5abcbd4d0cdab4b855970a6956f4f006c17117a3",
    ),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    bundle_rg()?;
    // fd is an optional vendored file-search binary backing a feature-gated
    // toolset; skip the download/embed entirely when that feature is off
    // (shipped TUI binaries).
    if env::var_os("CARGO_FEATURE_PI").is_some() {
        bundle_fd()?;
    }
    // bfs/ugrep back the bash-harness find/grep shadows (embedded_search_tools).
    bundle_search_tool("bfs", "BFS", BFS_VER)?;
    bundle_search_tool("ugrep", "UGREP", UGREP_VER)?;
    Ok(())
}

/// Download + embed fd as an optional vendored file-search binary, mirroring
/// the ripgrep bundling
/// (release-only or `GROW_TOOLS_BUNDLE_FD_PATH` override), plus pinned
/// per-asset SHA-256 verification of the downloaded tarball.
fn bundle_fd() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=GROW_TOOLS_BUNDLE_FD_PATH");
    println!("cargo:rustc-check-cfg=cfg(bundle_fd)");

    let gen_dir = PathBuf::from(env::var("OUT_DIR")?).join("bundle-fd");
    fs::create_dir_all(&gen_dir)?;

    // The consuming vendor extraction is unix-only — never bundle on
    // Windows targets, mirroring the bfs/ugrep skip.
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        return Ok(());
    }

    let path_override = env::var("GROW_TOOLS_BUNDLE_FD_PATH").ok();
    let is_release = env::var("PROFILE").as_deref() == Ok("release");
    if path_override.is_none() && !is_release {
        return Ok(());
    }

    // Per-target version: macOS x86_64 pins the last release with that asset.
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let (ver, asset_triple) = match (target_os.as_str(), target_arch.as_str()) {
        ("macos", "aarch64") => (FD_VER, "aarch64-apple-darwin"),
        ("macos", "x86_64") => (FD_VER_MACOS_X64, "x86_64-apple-darwin"),
        ("linux", "x86_64") => (FD_VER, "x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => (FD_VER, "aarch64-unknown-linux-musl"),
        _ => {
            if path_override.is_none() {
                return Err(format!(
                    "Unsupported target for fd bundling: {target_os}-{target_arch}. Set GROW_TOOLS_BUNDLE_FD_PATH to a local fd binary for offline or unsupported builds.",
                )
                .into());
            }
            (FD_VER, "override")
        }
    };

    println!("cargo:rustc-cfg=bundle_fd");
    println!("cargo:rustc-env=GROW_TOOLS_FD_VER={ver}");

    if let Some(path) = path_override {
        let dest = gen_dir.join(format!("fd-{ver}-override.bin"));
        println!("cargo:rustc-env=GROW_TOOLS_FD_TARGET=override");
        let _ = fs::remove_file(&dest);
        fs::copy(PathBuf::from(path.clone()), &dest).map_err(|e| {
            format!(
                "Failed copying GROW_TOOLS_BUNDLE_FD_PATH: {e} from path {path} to dest {}",
                dest.display()
            )
        })?;
        return Ok(());
    }

    println!("cargo:rustc-env=GROW_TOOLS_FD_TARGET={asset_triple}");
    let dest = gen_dir.join(format!("fd-{ver}-{asset_triple}.bin"));
    let _ = fs::remove_file(&dest);

    let url = format!(
        "https://github.com/sharkdp/fd/releases/download/v{ver}/fd-v{ver}-{asset_triple}.tar.gz"
    );

    let bytes: Vec<u8> = {
        let resp = reqwest::blocking::get(&url).map_err(|e| {
            format!(
                "Failed to download fd: {e}\nSet GROW_TOOLS_BUNDLE_FD_PATH to a local fd for offline builds."
            )
        })?;
        if !resp.status().is_success() {
            return Err(format!(
                "HTTP {} downloading fd. Set GROW_TOOLS_BUNDLE_FD_PATH for offline builds.",
                resp.status()
            )
            .into());
        }
        resp.bytes()?.to_vec()
    };

    // Verify the tarball against the pinned per-asset hash before unpacking.
    let expected_sha = FD_TARBALL_SHA256
        .iter()
        .find(|(v, t, _)| *v == ver && *t == asset_triple)
        .map(|(_, _, sha)| *sha)
        .ok_or_else(|| format!("No pinned SHA-256 for fd {ver} {asset_triple}"))?;
    let actual_sha = {
        use sha2::Digest as _;
        let mut hasher = sha2::Sha256::new();
        hasher.update(&bytes);
        hex_encode(&hasher.finalize())
    };
    if actual_sha != expected_sha {
        return Err(format!(
            "SHA-256 mismatch for {url}:\n  expected {expected_sha}\n  actual   {actual_sha}"
        )
        .into());
    }

    let gz = flate2::read::GzDecoder::new(&bytes[..]);
    let mut ar = tar::Archive::new(gz);
    let mut found = false;
    for entry in ar.entries()? {
        let mut e = entry?;
        let p = e.path()?;
        if p.file_name().is_some_and(|n| n == "fd") {
            let data: Vec<u8> = {
                let mut v = Vec::new();
                std::io::copy(&mut e, &mut v)?;
                v
            };
            fs::write(&dest, &data)?;
            found = true;
            break;
        }
    }

    if !found {
        return Err(format!(
            "Could not find 'fd' in fd archive {url}. Set GROW_TOOLS_BUNDLE_FD_PATH for offline builds."
        )
        .into());
    }

    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Bundle a prebuilt **static** search-tool binary (`bfs`/`ugrep`) when
/// `GROW_TOOLS_BUNDLE_<NAME>_PATH` points at one (supplied by the release
/// pipeline). Emits
/// `cfg(bundle_<name>)` so the crate's `include_bytes!` + self-extract engages.
///
/// bfs/ugrep publish no prebuilt static release assets, so the release pipeline
/// supplies the path. Unset → not
/// bundled (the runtime resolver falls back to `~/.grow/vendor` / `$PATH`);
/// never a hard failure, so an un-wired build still succeeds.
fn bundle_search_tool(
    name: &str,
    name_uc: &str,
    ver: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let override_env = format!("GROW_TOOLS_BUNDLE_{name_uc}_PATH");
    println!("cargo:rerun-if-env-changed={override_env}");
    // Always declare the cfg so `#[cfg(bundle_<name>)]` is lint-clean when unset.
    println!("cargo:rustc-check-cfg=cfg(bundle_{name})");

    // The consumer (`embedded_search_tools`) is `#[cfg(unix)]`, so embedding on a
    // Windows target is dead weight — skip (mirrors the ripgrep Windows skip).
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        return Ok(());
    }

    let Some(src) = env::var(&override_env).ok().filter(|s| !s.is_empty()) else {
        return Ok(());
    };

    let gen_dir = PathBuf::from(env::var("OUT_DIR")?).join(format!("bundle-{name}"));
    fs::create_dir_all(&gen_dir)?;
    let dest = gen_dir.join(format!("{name}-{ver}-override.bin"));
    let _ = fs::remove_file(&dest);
    fs::copy(&src, &dest)
        .map_err(|e| format!("copy {override_env} from {src} to {}: {e}", dest.display()))?;

    println!("cargo:rustc-cfg=bundle_{name}");
    println!("cargo:rustc-env=GROW_TOOLS_{name_uc}_VER={ver}");
    println!("cargo:rustc-env=GROW_TOOLS_{name_uc}_TARGET=override");
    Ok(())
}

/// Embed a ripgrep binary supplied by the release pipeline.
///
/// Keeping acquisition outside Cargo's build script makes local builds
/// deterministic and lets CI verify the downloaded asset before embedding it.
/// When unset, runtime resolution falls back to `rg` on PATH.
fn bundle_rg() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=GROW_TOOLS_BUNDLE_RG_PATH");
    println!("cargo:rustc-check-cfg=cfg(bundle_rg)");

    let Some(source) = env::var("GROW_TOOLS_BUNDLE_RG_PATH")
        .ok()
        .filter(|path| !path.is_empty())
    else {
        return Ok(());
    };

    // The release pipeline validates native sidecars on the runner and also
    // builds in containers that may not be able to execute the target binary.
    // Skip the redundant build-script exec check when the pipeline says so.
    let skip_exec_check = env::var_os("GROW_TOOLS_BUNDLE_RG_SKIP_EXEC_CHECK").is_some();

    let target = env::var("TARGET")?;
    if !skip_exec_check && env::var("HOST").as_deref() == Ok(target.as_str()) {
        let output = Command::new(&source)
            .arg("--version")
            .env_clear()
            .output()
            .map_err(|error| format!("failed to execute bundled ripgrep at {source}: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "bundled ripgrep at {source} is not runnable for target {target}: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
    }

    let gen_dir = PathBuf::from(env::var("OUT_DIR")?).join("bundle-rg");
    fs::create_dir_all(&gen_dir)?;
    let dest = gen_dir.join(format!("rg-{RG_VER}-{target}.bin"));
    let _ = fs::remove_file(&dest);
    fs::copy(&source, &dest).map_err(|error| {
        format!(
            "failed to copy GROW_TOOLS_BUNDLE_RG_PATH from {source} to {}: {error}",
            dest.display()
        )
    })?;

    println!("cargo:rustc-cfg=bundle_rg");
    println!("cargo:rustc-env=GROW_TOOLS_RG_VER={RG_VER}");
    println!("cargo:rustc-env=GROW_TOOLS_RG_TARGET={target}");
    Ok(())
}
