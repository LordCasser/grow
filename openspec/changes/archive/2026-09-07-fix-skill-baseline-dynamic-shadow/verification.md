## 复现
旧实现 refreshed_baseline_supersedes_same_path_discovery 失败：期望 fresh baseline / enabled=false，实际仍为动态旧描述 / enabled=true。使用真实临时文件路径。

## 修复验证
完整 Cargo tools lib skill_discovery_tracker：88 项全部通过，包含同路径刷新、条件门控、动态保留和前项 metadata 回归。registry 的 test_startup_skills_survive_dynamic_discovery 单独 1 项通过。

统一构建环境：CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216；cargo test --locked --offline -p tools --lib <filter> --quiet。

定向 git diff --check 通过。target 约 3.8 GiB，磁盘余约 77 GiB。无 API 或持久化格式变更，未运行 UI 端到端测试。
