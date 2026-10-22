# Prepare Grow 2.5.0

## Why

用户要求全部当前修改进入 2.5.0，先提交并清理工作区，再在本机独立 GROW_HOME 编译与验证周边功能，准备新版本。

## What Changes

更新 workspace/本地 lock 包版本与发行说明，提交当前完整工作树。以固定提交和 Rust 1.93.1 进行默认 CLI、workspace all-targets、核心库、周边 integration 与真实 CLI/PTY 回归，并保留版本身份、隔离环境及失败路径证据。

## Boundary

纯版本与发布准备维护，skip_specs；行为契约由既有归档负责，不创造新 delta。测试发现的问题单独定位，必要时在此记录最小修复，不借机继续优化架构。不提前开放输入、不拆分历史分页。本项只完成本地提交和发布准备，不创建/推送 tag 或触发公开发行。
