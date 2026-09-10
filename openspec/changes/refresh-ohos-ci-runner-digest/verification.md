# Verification

- Registry descriptor lookup: `docker manifest inspect --verbose swr.cn-north-4.myhuaweicloud.com/harmonybrew/ci-runner:latest` returned `sha256:cc0ca87c7bbb942a07fa0e774903064571af33400e5e4f5abce010551a272660` without pulling the image.
- Release run [34499720022](https://github.com/LordCasser/grow/actions/runs/34499720022) validated the tag and all non-OHOS jobs continued; the OHOS job failed before build because the former digest `c4be5fb8…` returned `manifest unknown`.
