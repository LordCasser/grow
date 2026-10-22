# Design

全部现有源码、测试、文档与归档一起纳入用户指定的 2.5.0。先更新本地 package versions 和发行说明，再提交；确认 Git 工作区干净后才开始测试。编译固定仓库 Rust 1.93.1，所有测试/CLI 命令在独立 HOME/GROW_HOME 下启动，保留显式 CARGO_HOME/RUSTUP_HOME 以复用工具链，沿用既有临时 sandbox/mock provider，不使用个人模型配置。

使用独立 target 并在完成后清理编译缓存，仅保留可供本机试用的 release-dist 二进制、摘要和验证日志。测试日志先写工作区外；结束后将精简证据归入本 change，归档并提交记录。核心库使用 debug assertions，避免 release-only 应 panic 用例无法覆盖。实际 CLI/PTY 使用本次 release-dist 二进制。全部现有 CI 已登记回归和受影响 crate integration 优先，按 host 标记跨平台限制。不用旧优化基准代替本轮当前提交验证。

正式发行仍需清洁、带匹配 annotated tag 的提交和该 SHA 的托管多平台门禁；本机验证不等于公开发行完成。
