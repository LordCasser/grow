## Decision record

不实施本次升级。Grow 当前依赖版本族保持一致，0.6.1；将依赖设为 0.7 会通过 caret 约束解析 0.7 系列，因此须验证实际锁定版本，而不是只看 manifest 的主次版本。

上游 issue #166 报告 0.7 系列在 x86_64 Linux musl 构建失败。Grow 的发布矩阵将 jemalloc 默认带入两个 Linux musl 发行目标，故此风险直接命中正式构建边界。审计机器没有可用 Linux musl 构建宿主，且本次明确不运行 Cargo；无法取得目标构建结果。不能以 macOS 检查或上游其它调用方构建代替该证据。

重启条件：上游关闭/修复该问题并发布包含修复的版本，或 Grow 的 Linux musl CI 能针对候选版本成功完成两个已发布目标的 release-dist 构建。新 change 再覆盖 allocator 运行时路径与平台矩阵；本审计不扩大为实现工作。

## Scope

仅记录依赖升级的可行性审计。保留现有依赖、配置、运行时 hooks 及发布行为，不更新产品规范。
