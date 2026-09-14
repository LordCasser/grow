## Why

OHOS release builds use a digest-pinned Harmonybrew `ci-runner` image, but the
SWR registry garbage-collects superseded manifests. A valid workflow can
therefore fail before compilation even though the same `latest` tag is
available.

## What Changes

- Use the existing Harmonybrew `ci-runner:latest` tag for the OHOS build.
- Always pull that tag before starting the container so a stale runner-local
  image is not reused.
- Keep the Harmonybrew core commit and Rust version checks in
  `scripts/build-ohos.sh`; the image reference is the only release-pipeline
  change.

This is CI tool maintenance only. It does not change the product, release
asset contract, signing gates, or supported target set, so `skip_specs: true`.

## Impact

`.github/workflows/build-one.yml` and this maintenance record. The image
contents remain an external dependency and are intentionally resolved at
workflow execution time; the build script retains the in-repository toolchain
pin that is under Grow's control.
