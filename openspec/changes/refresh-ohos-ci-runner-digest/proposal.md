# Why

The v2.1.6 release workflow reached all platform jobs, but the OHOS job could not pull its pinned Harmonybrew CI image because the registry garbage-collected that digest. The source and OHOS build contract are unchanged; the release is blocked by stale external image metadata.

# What changes

Refresh the pinned `ci-runner` digest to the manifest currently published at the existing registry tag, and record the lookup and failed run. No product behavior or target set changes.

# Capabilities

This is a CI tool maintenance change, so it skips product specs. The workflow remains digest-pinned and will fail closed when the exact image is unavailable.
