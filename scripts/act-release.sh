#!/usr/bin/env bash
# Local verification of .github/workflows/release.yml via nektos/act.
#
# Usage:
#   scripts/act-release.sh                  # validate job only (fast)
#   scripts/act-release.sh validate
#   scripts/act-release.sh build            # linux-x86_64 matrix entry
#   scripts/act-release.sh build <asset>    # another Linux matrix entry
#   scripts/act-release.sh publish          # needs prior artifacts; dry upload
#   scripts/act-release.sh list
#
# Requires: act, docker, gh (optional, for GITHUB_TOKEN), and a committed tree.
# A missing release tag is created locally at clean HEAD and is never pushed.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TAG="${ACT_RELEASE_TAG:-v1.1.2}"
EVENT="${ROOT}/.github/act/workflow_dispatch.json"
WF="${ROOT}/.github/workflows/release.yml"
JOB="${1:-validate}"
ASSET_PLATFORM="${2:-linux-x86_64}"

if ! command -v act >/dev/null; then
  echo "act not found; install: brew install act" >&2
  exit 1
fi
if [[ "$JOB" == "list" ]]; then
  act -W "$WF" -l
  exit 0
fi
if ! docker info >/dev/null 2>&1; then
  echo "docker is not running" >&2
  exit 1
fi

# Keep event payload tag in sync with ACT_RELEASE_TAG.
tmp_event="$(mktemp)"
trap 'rm -f "$tmp_event"' EXIT
python3 - "$EVENT" "$TAG" "$tmp_event" <<'PY'
import json, sys
src, tag, dst = sys.argv[1], sys.argv[2], sys.argv[3]
with open(src) as f:
    data = json.load(f)
data.setdefault("inputs", {})["tag"] = tag
with open(dst, "w") as f:
    json.dump(data, f)
PY

# act checkout needs the release tag to resolve in this clone.
if ! git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  if [[ -n "$(git status --porcelain)" ]]; then
    echo "Refusing to create ${TAG}: commit or stash the release tree first" >&2
    git status --short >&2
    exit 1
  fi
  expected="${TAG#v}"
  actual="$(
    cargo metadata --locked --no-deps --format-version 1 \
      | python3 -c "import json,sys; d=json.load(sys.stdin); print(next(p['version'] for p in d['packages'] if p['name']=='cli'))"
  )"
  if [[ "$actual" != "$expected" ]]; then
    echo "Refusing to create ${TAG}: cli is ${actual}, expected ${expected}" >&2
    exit 1
  fi
  echo "Creating local tag ${TAG} at HEAD for act checkout (not pushed)"
  git tag -a "$TAG" -m "act local release tag ${TAG}"
fi

token="${GITHUB_TOKEN:-}"
if [[ -z "$token" ]] && command -v gh >/dev/null; then
  token="$(gh auth token 2>/dev/null || true)"
fi

# --bind: use the local clone (needed for unpushed tags / dirty tree).
# Workflow skips actions/checkout when ACT=true so the bind mount is not wiped.
# Explicit -P so act never silently skips ubuntu-24.04.
common=(
  act workflow_dispatch
  -W "$WF"
  -e "$tmp_event"
  --bind
  --env "RELEASE_TAG=${TAG}"
  --env "ACT=true"
  --container-architecture linux/arm64
  -P "ubuntu-24.04=catthehacker/ubuntu:act-latest"
  -P "ubuntu-24.04-arm=catthehacker/ubuntu:act-latest"
)
if [[ -n "$token" ]]; then
  common+=(-s "GITHUB_TOKEN=${token}")
fi

case "$JOB" in
  validate)
    "${common[@]}" -j validate
    ;;
  build)
    case "$ASSET_PLATFORM" in
      linux-x86_64|linux-riscv64|linux-x86_64-musl|linux-aarch64-musl)
        container_arch=linux/amd64
        ;;
      linux-aarch64)
        container_arch=linux/arm64
        ;;
      *)
        echo "act build supports Linux entries only; got: $ASSET_PLATFORM" >&2
        exit 1
        ;;
    esac
    build_args=(
      act workflow_dispatch
      -W "$WF"
      -e "$tmp_event"
      --bind
      --env "RELEASE_TAG=${TAG}"
      --env "ACT=true"
      --container-architecture "$container_arch"
      -P "ubuntu-24.04=catthehacker/ubuntu:act-latest"
      -P "ubuntu-24.04-arm=catthehacker/ubuntu:act-latest"
      -j build
      --matrix "asset_platform:${ASSET_PLATFORM}"
    )
    if [[ -n "$token" ]]; then
      build_args+=(-s "GITHUB_TOKEN=${token}")
    fi
    "${build_args[@]}"
    ;;
  publish)
    "${common[@]}" -j publish
    ;;
  *)
    echo "Unknown job: $JOB (validate|build [linux asset]|publish|list)" >&2
    exit 1
    ;;
esac
