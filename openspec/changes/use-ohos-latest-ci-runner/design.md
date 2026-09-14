## Decision

Set `CI_RUNNER` to
`swr.cn-north-4.myhuaweicloud.com/harmonybrew/ci-runner:latest` and invoke
`docker run --pull always`. This removes the failure mode caused by a
garbage-collected digest and prevents an old locally cached tag from masking a
newly published image.

The OHOS build script continues to pin Harmonybrew core and require Rust
1.98.0. No new fallback image, retry loop, signing path, or compatibility
branch is introduced.

## Verification

Validate the workflow shell/YAML structure, run strict OpenSpec validation,
and rerun the full release workflow. Publication remains gated on all ten
assets and the existing OHOS strip, smoke, signing, and size checks.
