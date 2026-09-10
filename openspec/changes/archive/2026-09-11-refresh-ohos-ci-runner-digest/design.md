# Design

Read the current `latest` manifest descriptor without pulling the image, update only `CI_RUNNER` and its verification date, then rerun the full release workflow after retagging the unreleased commit. Keep the previous failed run as evidence of the registry failure. Do not alter OHOS build commands, signing, asset names, or publication gates.
