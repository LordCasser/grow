#!/usr/bin/env bash
# build-ohos.sh — Phase 1: build grow for OpenHarmony (aarch64-unknown-linux-ohos)
# inside the Harmonybrew ci-runner container (OHOS userland, arm64).
#
# Verified pitfalls this script handles (2026-08-02, docs/ohos-porting.md §6/§7b):
#   1. OHOS *host* Rust toolchains only ship from 1.93; brew's `rust` formula
#      (official rust-<ver>-aarch64-unknown-linux-ohos dist, rpath-patched for
#      libssl/libcrypto/libz) is the working route. It is linked as rustup
#      toolchain "system" (the formula's own caveat).
#   2. rustup on OHOS misdetects the host as aarch64-unknown-linux-musl, whose
#      rustc needs libgcc_s/_Unwind_* (absent on OHOS musl) — never use it.
#      rustup-init is installed with --default-toolchain none to avoid pulling it.
#   3. cargo's TLS (libcurl+brew openssl) intermittently fails against
#      crates.io and the USTC mirror ("unexpected eof"); rsproxy.cn was verified
#      5/5 vs USTC 0/5 for downloads. The script installs the rsproxy sparse
#      mirror unless the user already configured a mirror. Downloads are wrapped
#      in a bounded retry loop (CARGO_NET_RETRY + outer attempts).
#   4. C deps need the OHOS SDK clang (aarch64-unknown-linux-ohos-clang) +
#      sysroot; aws-lc-sys additionally needs OHOS_NDK_HOME + the SDK's cmake.
#   5. jemalloc's configure doesn't know the `aarch64-unknown-linux-ohos`
#      triplet -> build with --no-default-features --features sandbox-enforce.
#   6. nix 0.26.4 (pprof -> shell-base chain) predates OHOS; the repo vendors
#      the patched `third_party/nix-ohos` via [patch.crates-io]. nix 0.28/0.30
#      support OHOS natively and stay on the registry.
#   7. The resulting binary links libz.so + libtime_service_ndk.so at runtime
#      (SDK clang defaults). Minimal rootfs (DockerHarmony/ci-runner) lacks
#      them under those names; --smoke copies them from the SDK sysroot.
#
# Host-side usage (arm64 docker host):
#   docker run -itd --name grow-ohos-ci \
#     --mount type=bind,source="$PWD",target=/workspace/grow \
#     swr.cn-north-4.myhuaweicloud.com/harmonybrew/ci-runner:latest sh
#   docker exec grow-ohos-ci /workspace/grow/scripts/build-ohos.sh [--smoke]
#
# Env overrides:
#   OHOS_SDK_ROOT            SDK root (default /opt/ohos-sdk/ohos)
#   GROW_VERSION             version stamp for VERSION_WITH_COMMIT
#                            (default: [workspace.package].version)
#   GROW_TOOLS_BUNDLE_RG_PATH  embed this OHOS rg binary (optional; else PATH rg)
#   CARGO_PROFILE            release (default) | release-dist
#   CARGO_TARGET_DIR         (default $HOME/grow-target; container-local so the
#                            host workspace mount is not polluted)
#   RUSTUP_HOME / CARGO_HOME (default $HOME/.rustup / $HOME/.cargo)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# ---------------------------------------------------------------- sanity ----
if [ "$(uname -m)" != "aarch64" ] || [ ! -e /lib/ld-musl-aarch64.so.1 ]; then
  echo "error: build-ohos.sh must run inside the OHOS ci-runner container" >&2
  echo "  (arm64 userland with /lib/ld-musl-aarch64.so.1)." >&2
  echo "  See the header of this script for the docker invocation." >&2
  exit 1
fi

# ------------------------------------------------------------ defaults ----
OHOS_SDK_ROOT="${OHOS_SDK_ROOT:-/opt/ohos-sdk/ohos}"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/grow-target}"
CARGO_PROFILE="${CARGO_PROFILE:-release}"
RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME CARGO_HOME CARGO_TARGET_DIR

log() { printf '\n=== %s ===\n' "$*"; }

# -------------------------------------------------- toolchain bootstrap ----
log "Toolchain bootstrap (idempotent)"
if ! command -v brew >/dev/null 2>&1; then
  echo "error: brew not found; this script targets the Harmonybrew ci-runner image" >&2
  exit 1
fi
if ! brew --prefix rust >/dev/null 2>&1; then
  log "Installing rust via Harmonybrew (official OHOS-host dist, rpath-patched)"
  HOMEBREW_NO_AUTO_UPDATE=1 brew install rust
fi
if ! command -v rustup >/dev/null 2>&1 && [ ! -x "$CARGO_HOME/bin/rustup" ]; then
  log "Installing rustup (default toolchain none; the musl host rustc is broken on OHOS)"
  curl -fsSL --max-time 120 https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
fi
# shellcheck disable=SC1091
. "$CARGO_HOME/env" 2>/dev/null || true
export PATH="$CARGO_HOME/bin:$PATH"

log "Linking brew rust as rustup toolchain 'system'"
rustup toolchain link system "$(brew --prefix rust)"
export RUSTUP_TOOLCHAIN=system
cargo --version
rustc --version

# ----------------------------------------------------------- SDK env -------
log "OHOS SDK environment ($OHOS_SDK_ROOT)"
for d in \
  "$OHOS_SDK_ROOT/native/llvm/bin" \
  "$OHOS_SDK_ROOT/native/build-tools/cmake/bin" \
  "$OHOS_SDK_ROOT/native/sysroot/usr"; do
  if [ ! -d "$d" ]; then
    echo "error: OHOS SDK layout not found at $OHOS_SDK_ROOT (missing $d)" >&2
    exit 1
  fi
done
export OHOS_NDK_HOME="$OHOS_SDK_ROOT"
export PATH="$OHOS_SDK_ROOT/native/llvm/bin:$OHOS_SDK_ROOT/native/build-tools/cmake/bin:$PATH"
export CC_aarch64_unknown_linux_ohos=aarch64-unknown-linux-ohos-clang
export CXX_aarch64_unknown_linux_ohos=aarch64-unknown-linux-ohos-clang++
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER=aarch64-unknown-linux-ohos-clang

# ----------------------------------------------------- cargo mirror -------
# crates.io / USTC TLS is flaky from this environment; rsproxy verified stable.
# Respect an existing mirror configuration instead of clobbering it.
log "Cargo registry mirror"
CONFIG="$CARGO_HOME/config.toml"
if [ -f "$CONFIG" ] && grep -q "replace-with" "$CONFIG"; then
  echo "existing mirror config kept: $CONFIG"
else
  if [ -f "$CONFIG" ]; then
    cp "$CONFIG" "$CONFIG.bak"
    echo "backed up existing config to $CONFIG.bak"
  fi
  mkdir -p "$CARGO_HOME"
  cat > "$CONFIG" <<'EOF'
[source.crates-io]
replace-with = "rsproxy-sparse"

[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"

[registries.rsproxy-sparse]
index = "sparse+https://rsproxy.cn/index/"
EOF
  echo "installed rsproxy sparse mirror"
fi
export CARGO_HTTP_MULTIPLEXING=false
export CARGO_NET_RETRY=8

# ------------------------------------------------------- embed rg ---------
if [ -n "${GROW_TOOLS_BUNDLE_RG_PATH:-}" ]; then
  if [ ! -x "$GROW_TOOLS_BUNDLE_RG_PATH" ]; then
    echo "error: GROW_TOOLS_BUNDLE_RG_PATH set but not executable: $GROW_TOOLS_BUNDLE_RG_PATH" >&2
    exit 1
  fi
  export GROW_TOOLS_BUNDLE_RG_PATH
  # HOST == TARGET inside the container, so the build-script exec check would
  # run; the pipeline may supply a binary that is not runnable on the runner.
  export GROW_TOOLS_BUNDLE_RG_SKIP_EXEC_CHECK=1
  echo "embedding rg from: $GROW_TOOLS_BUNDLE_RG_PATH"
else
  # OHOS releases intentionally do NOT bundle rg: runtime resolution falls
  # back to `rg` on PATH (installed via Harmonybrew), and search tools error
  # with an install hint when it is missing.
  echo "no GROW_TOOLS_BUNDLE_RG_PATH: runtime rg falls back to PATH"
fi

# ------------------------------------------------------------ build -------
# jemalloc's configure does not know the OHOS triplet; first version must
# build without it (docs/ohos-porting.md §3.7). sandbox-enforce stays on.
FEATURE_FLAGS=(--no-default-features --features sandbox-enforce)
LOG=/tmp/build-ohos.log

log "cargo build (profile=$CARGO_PROFILE, target=aarch64-unknown-linux-ohos)"
echo "log: $LOG"
# First pass without --locked: refresh Cargo.lock if [patch.crates-io] entries
# (third_party/nix-ohos) are newer than the lockfile.
if cargo build --profile "$CARGO_PROFILE" --locked -p cli --bin grow \
    "${FEATURE_FLAGS[@]}" >"$LOG" 2>&1; then
  :
else
  echo "locked build failed (refreshing lock once, then retrying): $(grep -E '^error' "$LOG" | head -2)"
  cargo build --profile "$CARGO_PROFILE" -p cli --bin grow \
      "${FEATURE_FLAGS[@]}" >"$LOG" 2>&1 || true
fi

ok=0
for attempt in 1 2 3 4 5; do
  echo "--- attempt $attempt ---"
  if cargo build --profile "$CARGO_PROFILE" --locked -p cli --bin grow \
      "${FEATURE_FLAGS[@]}" >>"$LOG" 2>&1; then
    ok=1
    break
  fi
  err="$(grep -E '^error' "$LOG" | head -3)"
  echo "attempt $attempt failed: $err"
  if grep -q "SSL connect\|spurious network" "$LOG"; then
    echo "  (network error; mirror/retry handles it)"
  fi
done
if [ "$ok" -ne 1 ]; then
  echo "build failed after 5 attempts; tail of $LOG:" >&2
  tail -30 "$LOG" >&2
  exit 1
fi

BIN="$CARGO_TARGET_DIR/release/grow"
if [ ! -x "$BIN" ]; then
  echo "error: binary not found at $BIN" >&2
  exit 1
fi

log "Artifact"
ls -lh "$BIN"
readelf -l "$BIN" 2>/dev/null | grep -E "interpreter" | head -1 || true
readelf -d "$BIN" 2>/dev/null | grep NEEDED | head -8 || true

# ------------------------------------------------------------ smoke --------
if [ "${1:-}" = "--smoke" ]; then
  log "Smoke (container): runtime libs + --version + sessions + doctor"
  SYSROOT_LIB="$OHOS_SDK_ROOT/native/sysroot/usr/lib/aarch64-linux-ohos"
  for lib in libz.so libtime_service_ndk.so; do
    if [ ! -e "/lib64/$lib" ] && [ -e "$SYSROOT_LIB/$lib" ]; then
      cp "$SYSROOT_LIB/$lib" "/lib64/$lib"
      chmod 755 "/lib64/$lib"
      echo "installed runtime lib: /lib64/$lib (from SDK sysroot)"
    fi
  done
  export TERM=xterm-256color
  export HOME="${HOME:-/storage/Users/currentUser}"
  "$BIN" --version
  GROW_HOME="$HOME/.grow-smoke" "$BIN" sessions list
  GROW_HOME="$HOME/.grow-smoke" "$BIN" doctor 2>&1 | head -10 || true
fi

log "Done. Binary: $BIN"
echo "Next: commit third_party/nix-ohos + Cargo.toml/Cargo.lock (if not yet),"
echo "      then see docs/ohos-porting.md §9 for the roadmap (updater,"
echo "      release matrix, Harmonybrew formula, real-device gates)."
