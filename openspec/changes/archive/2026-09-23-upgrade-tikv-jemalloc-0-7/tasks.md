## 1. Feasibility decision

- [x] 1.1 核对本地三项 jemalloc 直接依赖、锁定版本、CLI 默认 feature 与正式 Linux musl 发布目标。
- [x] 1.2 核对上游 0.7 系列 musl 构建问题；记录 issue URL、报告版本、日期和审计时状态。
- [x] 1.3 在未能验证两个发布目标时决定暂缓升级，记录准确的重新启动条件；不改 `Cargo.toml`、`Cargo.lock` 或运行时代码。
- [x] 1.4 记录验证范围，运行 OpenSpec 严格校验并按 skip-specs 归档。
