# Verification

- `docker manifest inspect --verbose swr.cn-north-4.myhuaweicloud.com/harmonybrew/ci-runner:latest` confirmed the current multi-platform manifest is available.
- `openspec validate --all --strict --no-interactive`, `git diff --check`, Bash syntax checks, and Ruby YAML parsing passed before commit.
- Commit `dc1f2eb9` changed the OHOS workflow to `ci-runner:latest` with `docker run --pull always` and was pushed to `main`.
- Release run [34854284028](https://github.com/LordCasser/grow/actions/runs/34854284028) completed successfully. All ten platform jobs passed, including `ohos-aarch64`.
- The non-draft, non-prerelease [v2.1.7 GitHub Release](https://github.com/LordCasser/grow/releases/tag/v2.1.7) was published with ten platform archives and `SHA256SUMS` (11 assets total).
