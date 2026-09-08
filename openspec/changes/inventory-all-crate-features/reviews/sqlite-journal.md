# sqlite-journal 逐包核查

包路径：`crates/codegen/sqlite-journal`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本批尚未运行动态测试。

## 模块与开关

- `crates/codegen/sqlite-journal/Cargo.toml`
- `crates/codegen/sqlite-journal/src/lib.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Filesystem selected journal mode](../specs/sqlite-storage/spec.md#requirement-filesystem-selected-journal-mode)：SQLite journal 选择 SHALL 检查数据库父目录，GROW_SQLITE_JOURNAL_MODE 可用大小写无关 wal/truncate 覆盖；空值不覆盖，非法值警告后继续探测。
- [Per host rollback database path](../specs/sqlite-storage/spec.md#requirement-per-host-rollback-database-path)：TRUNCATE 模式 SHALL 使用基于规范化 hostname 的同目录 .h-<host> 文件名，保留扩展名且重复转换幂等；WAL 不改路径。
- [Safe journal conversion and readonly opening](../specs/sqlite-storage/spec.md#requirement-safe-journal-conversion-and-readonly-opening)：连接打开 SHALL 先配置 5 秒 busy timeout 再设置 journal；TRUNCATE 转换按 EXCLUSIVE → TRUNCATE → NORMAL 顺序执行。

## 边界

- 选择 TRUNCATE；本地、探测失败和没有实现探测的平台默认 WAL。
- 分别使用 statfs magic、MNT_LOCAL/类型名、UNC/remote drive 分类；Linux FUSE 也按 network 处理。
- 保留原路径但仍使用 TRUNCATE，不承诺不同主机一定无命名碰撞。
- effective_db_path 保持原值，不继续追加标签。
- 返回错误，不创建原路径或 per-host DB。
- 使用无 CREATE 的可写 FD 完成转换，然后 query_only 禁止 SQL 写；WAL 用只读 FD。
- 向调用方传播错误，不保证 busy_timeout 能覆盖每一条转换锁路径。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
