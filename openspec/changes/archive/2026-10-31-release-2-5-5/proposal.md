## Why

用户要求将当前改动分批提交、清理编译 warning，并发布新的最小补丁版本 2.5.5。这些修复已分别归档，需要保留各自提交边界，并让版本、标签、产物和验证记录指向同一发布源。

## What Changes

按授权、Plan、压缩生命周期、自动更新拆分既有变更；使用固定 Rust 1.93.1 核对默认 CLI 与 workspace all-targets 诊断，仅修复有编译证据的 warning；同步 workspace/lockfile/changelog，通过现有同 SHA 发布门禁发布十个平台。

## Contract

本 change 是版本与构建维护，不新增行为契约，skip_specs: true。各行为变化由本日既有归档负责。无关 backlog 不扩入发布。
