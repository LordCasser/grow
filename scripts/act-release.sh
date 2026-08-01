#!/usr/bin/env bash
# Local verification of .github/workflows/release.yml via nektos/act.
#
# Usage:
#   scripts/act-release.sh                  # validate job only (fast)
#   scripts/act-release.sh validate
#   scripts/act-release.sh build            # linux-aarch64 matrix only
#   scripts/act-release.sh publish          # needs prior artifacts; dry upload
#   scripts/act-release.sh list
#
# Requires: act, docker, gh (optional, for GITHUB_TOKEN), tag v1.0.0 matching
# workspace version (created locally if missing, not pushed).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TAG="${ACT_RELEASE_TAG:-v1.0.0}"
EVENT="${ROOT}/.github/act/workflow_dispatch.json"
WF="${ROOT}/.github/workflows/release.yml"
JOB="${1:-validate}"

if ! command -v act >/dev/null; then
  echo "act not found; install: brew install act" >&2
  exit 1
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
  list)
    act -W "$WF" -l
    ;;
  validate)
    "${common[@]}" -j validate
    ;;
  build)
    # Host is typically arm64; run the matching Linux matrix entry only.
    "${common[@]}" -j build --matrix asset_platform:linux-aarch64
    ;;
  build-amd64)
    # Drop arm64 default; force amd64 container + matrix entry.
    act workflow_dispatch \
      -W "$WF" \
      -e "$tmp_event" \
      --bind \
      --env "RELEASE_TAG=${TAG}" \
      --env "ACT=true" \
      --container-architecture linux/amd64 \
      -P "ubuntu-24.04=catthehacker/ubuntu:act-latest" \
      ${token:+-s "GITHUB_TOKEN=${token}"} \
      -j build --matrix asset_platform:linux-x86_64
    ;;
  publish)
    "${common[@]}" -j publish
    ;;
  *)
    echo "Unknown job: $JOB (validate|build|build-amd64|publish|list)" >&2
    exit 1
    ;;
esac
