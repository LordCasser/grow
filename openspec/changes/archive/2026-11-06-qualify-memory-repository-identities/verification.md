# Verification

父review并整合 `/tmp/ws04-qualify-memory-repository-identities.patch`，核对root Cargo.lock仅memory依赖列表增加url、既有workspace url版本不变。初始候选SCP改写遗漏authority/path分隔符，父发现后修为`ssh://{authority}/{path}`；原path前导斜杠形成双slash，保留与相对路径的既定隔离。新增git2 fixture因Tree借用未结束而E0505，修正作用域，不更改断言。

实际运行 `cargo +1.93.1 test --locked --offline -p memory --lib -- --test-threads=1`：**321 passed / 0 failed，exit0**。父核原始terminal日志：child `01a1af49-de91-7ac2-b2dd-90d30624c167` 的`call_8642b843008c4b21a3c5dd31.log`，实际running321，19.45秒测试运行。包含真实repo/origin/linkedworktree与MemoryStorage::new临时root：同host跨transport同目录、不同host/port隔离、默认port等价、旧hostless sentinel不采纳、不迁移或清除、new_flat和fallback保留。所有原storage/搜索/索引回归保留。

父核最终normalize实现只取host/非默认port/path，凭据与scheme不入identity，未知origin沿原cwd fallback。官方url组件及默认port语义由Context7 `/servo/rust-url`查询，与锁定包本机测试共同支持；不声称识别SSH配置中的主机别名或任意远端等价。此前hostless工作区目录可能已混有不同host事实，因此不自动归入新目录；构造保持惰性，不触碰旧目录。开发者说明同步。

边界：只在本机隔离存储运行，未运行真实用户memory、未进行网络访问或Windows平台实测；旧实现新增fixture未单独执行，不声称已有old-red实测。报告静态碰撞依据与当前green证据分别保留。
