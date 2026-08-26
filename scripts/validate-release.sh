#!/usr/bin/env bash
# Static preflight for the official GitHub release workflow. This intentionally
# does not emulate GitHub Actions or cross-compile targets: the workflow remains
# the only release executor, while this script verifies its immutable contracts.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

release_tag="${1:-v2.0.1}"
release_workflow=".github/workflows/release.yml"
build_workflow=".github/workflows/build-one.yml"

if [[ ! "$release_tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$ ]]; then
  echo "invalid release tag: $release_tag" >&2
  exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
  echo "release validation requires a clean committed tree" >&2
  git status --short >&2
  exit 1
fi
if [[ "$(git cat-file -t "refs/tags/${release_tag}" 2>/dev/null || true)" != "tag" ]]; then
  echo "release tag must exist locally and be annotated: $release_tag" >&2
  exit 1
fi

head_commit="$(git rev-parse HEAD)"
tag_commit="$(git rev-parse "refs/tags/${release_tag}^{commit}")"
if [[ "$tag_commit" != "$head_commit" ]]; then
  echo "release tag must resolve to HEAD: tag=$tag_commit HEAD=$head_commit" >&2
  exit 1
fi

version="$({ cargo metadata --locked --no-deps --format-version 1; } \
  | python3 -c 'import json, sys; data=json.load(sys.stdin); print(next(p["version"] for p in data["packages"] if p["name"] == "cli"))')"
if [[ "${release_tag#v}" != "$version" ]]; then
  echo "release tag $release_tag does not match cli / workspace $version" >&2
  exit 1
fi
release_notes="crates/codegen/shell/changelogs/${version}.md"
if [[ ! -s "$release_notes" ]]; then
  echo "release notes are missing or empty: $release_notes" >&2
  exit 1
fi

python3 - "$release_workflow" "$build_workflow" <<'PY'
import json
import pathlib
import re
import sys
import textwrap

release_path = pathlib.Path(sys.argv[1])
build_path = pathlib.Path(sys.argv[2])
release = release_path.read_text()
build = build_path.read_text()

expected_platforms = [
    "linux-x86_64",
    "linux-aarch64",
    "linux-riscv64",
    "linux-x86_64-musl",
    "linux-aarch64-musl",
    "macos-aarch64",
    "macos-x86_64",
    "windows-x86_64",
    "windows-aarch64",
    "ohos-aarch64",
]

catalog_match = re.search(
    r"cat <<'JSONEOF'[^\n]*\n(?P<catalog>.*?)\n\s*JSONEOF",
    release,
    re.DOTALL,
)
if catalog_match is None:
    raise SystemExit("release platform catalog heredoc is missing")
catalog = json.loads(textwrap.dedent(catalog_match.group("catalog")))
platforms = [entry.get("asset_platform") for entry in catalog]
if platforms != expected_platforms or len(platforms) != len(set(platforms)):
    raise SystemExit(f"release platform catalog mismatch: {platforms!r}")

required_blocks = re.findall(r"required=\(\n(?P<body>.*?)\n\s*\)", release, re.DOTALL)
if len(required_blocks) != 2:
    raise SystemExit(f"expected two exact publication asset lists, found {len(required_blocks)}")
asset_pattern = re.compile(r'"grow-\$\{version\}-(?P<platform>[^"$]+)\.tar\.gz"')
for index, block in enumerate(required_blocks, start=1):
    assets = asset_pattern.findall(block)
    if assets != expected_platforms:
        raise SystemExit(f"publication asset list {index} mismatch: {assets!r}")

updater_source = pathlib.Path("crates/codegen/update/src/version.rs").read_text()
updater_repo = re.search(r'^pub const GH_RELEASE_REPO: &str = "([^"]+)";', updater_source, re.MULTILINE)
workflow_repo = re.search(r'^\s*RELEASE_REPO:\s*([^\s#]+)', release, re.MULTILINE)
if updater_repo is None or workflow_repo is None or updater_repo.group(1) != workflow_repo.group(1):
    raise SystemExit("workflow RELEASE_REPO does not match updater GH_RELEASE_REPO")
if workflow_repo.group(1) != "LordCasser/grow":
    raise SystemExit(f"unexpected official release repository: {workflow_repo.group(1)!r}")

release_contracts = [
    "release_commit: ${{ needs.validate.outputs.commit }}",
    'git cat-file -t "refs/tags/${RELEASE_TAG}"',
    'git ls-remote --tags "https://github.com/${RELEASE_REPO}.git"',
    'git ls-remote --tags "https://github.com/${GH_REPO}.git"',
    '--signer-workflow "LordCasser/grow/.github/workflows/build-one.yml"',
    "SHA256SUMS",
]
build_contracts = [
    "ref: ${{ env.RELEASE_COMMIT }}",
    "--features release-dist",
    "uses: actions/attest@v4",
    "codesign --force --sign -",
    "codesign --verify --strict",
]
for contract in release_contracts:
    if contract not in release:
        raise SystemExit(f"release workflow contract is missing: {contract}")
for contract in build_contracts:
    if contract not in build:
        raise SystemExit(f"build workflow contract is missing: {contract}")
if re.search(r"\bACT\b|act-release", release + build):
    raise SystemExit("official workflows must not carry a local ACT execution branch")
PY

bash -n scripts/build-ohos.sh scripts/validate-release.sh
if command -v ruby >/dev/null 2>&1; then
  ruby -e 'require "yaml"; ARGV.each { |path| YAML.parse_file(path) }' \
    "$release_workflow" "$build_workflow"
fi
git diff --check

printf 'validated release %s (%s) at %s\n' "$release_tag" "$version" "$head_commit"
