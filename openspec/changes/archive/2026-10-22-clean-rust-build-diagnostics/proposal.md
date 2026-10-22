# Rust 构建诊断与格式整理

## Why

发布 2.5.0 前需要核对固定工具链的编译警告和 workspace 格式，避免宿主工具链版本差异造成验证误读。

## What Changes

运行 cargo fmt 并补齐开发入口说明，记录 workspace all-targets、默认 CLI check/build 和 Clippy 原始诊断。保留产品行为、持久化与接口契约，纯格式/非行为维护，skip_specs。Clippy 存量按用户授权另拆 change 全量清理；不通过 blanket allow、关闭诊断或修改失败断言换取通过。
