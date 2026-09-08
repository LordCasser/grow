## nix-ohos crate 全量静态审计

完成 `third_party/nix-ohos` 全量静态审计：62个文件（58个 Rust、2个 Cargo manifest、README、LICENSE），31,878行源码，形成62条 `sandbox-boundary`、`process-lifecycle`、`developer-support`、`configuration-rules` 和 `filesystem-events` 需求。全部路径、行数、SHA-256、测试属性和来源符号已核验；未运行 Cargo 或平台系统调用。
