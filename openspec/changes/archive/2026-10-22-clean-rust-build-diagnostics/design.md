# Design

先保留 fmt/check 原始诊断，以仓库固定 Rust 1.93.1 校验默认 CLI、workspace all-targets 和链接，核对本机 PATH 中 Homebrew 1.99.1 的额外诊断。复用已有 lint 配置与 CI 入口，Clippy 诊断单独区分。只作等价维护，逐项核对调用与失败路径；无关架构债务独立记录。所有编译/测试在独立 HOME/GROW_HOME 和专用 target 下执行，完成后清理该 target；不清理共享缓存或用户状态。
