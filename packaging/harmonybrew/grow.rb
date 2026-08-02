# Harmonybrew formula for Grow (OpenHarmony / HarmonyOS PC).
#
# Work-in-progress for the Harmonybrew/homebrew-core contribution
# (docs/ohos-porting.md §9 Step 3). Verified 2026-08-02 in the ci-runner
# container: `brew install -s --include-test grow` builds in ~14 min with
# the stock superenv environment (no cmake/OHOS SDK env needed since
# aws-lc-rs was removed from the graph; the SDK clang is found via the
# /bin cc shims).
#
# NOTE: the `url`/`sha256` below are LOCAL TEST values (a git-archive of the
# ohos-adaptation branch served over http://127.0.0.1:8000). The real PR must
# point at the official release tarball:
#   url "https://github.com/LordCasser/grow/archive/refs/tags/v1.1.0.tar.gz"
# with the matching sha256, once v1.1.0 (or the merged OHOS work) is tagged.
# Drop `depends_on "zsh"` only after the POSIX shell backend lands.
class Grow < Formula
  desc "Terminal-based AI coding agent with a Rust TUI (fork of xAI Grok Build)"
  homepage "https://github.com/LordCasser/grow"
  url "http://127.0.0.1:8000/grow-ohos-test.tar.gz"
  sha256 "fbf5f3b2cd056d8b03580e2d09d2ba8484659b520cb11f8eca42b7840ef71948"
  license "Apache-2.0"
  head "https://github.com/LordCasser/grow.git", branch: "ohos-adaptation"

  depends_on "rust" => :build
  depends_on "ripgrep"
  depends_on "zsh"

  def install
    # jemalloc's configure does not understand the aarch64-unknown-linux-ohos
    # triplet, so the first OpenHarmony release builds without it.
    # sandbox-enforce stays on; distro-pm disables self-update (brew owns
    # upgrades). aws-lc-rs was removed from the graph, so no cmake/OHOS SDK
    # environment is needed for crypto.
    system "cargo", "install", *std_cargo_args(path: "crates/codegen/cli"), "--bin", "grow",
           "--no-default-features", "--features", "sandbox-enforce,distro-pm"
  end

  test do
    assert_match "grow", shell_output("#{bin}/grow --version")
  end
end
