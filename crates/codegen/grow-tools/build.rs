//! Build script for bundling search tools for the grow-tools crate.
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    bundle_rg()?;
    // bfs/ugrep back the bash-harness find/grep shadows (embedded_search_tools).
    bundle_search_tool("bfs", "BFS", BFS_VER)?;
    bundle_search_tool("ugrep", "UGREP", UGREP_VER)?;
    Ok(())
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

    let target = env::var("TARGET")?;
    if env::var("HOST").as_deref() == Ok(target.as_str()) {
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
