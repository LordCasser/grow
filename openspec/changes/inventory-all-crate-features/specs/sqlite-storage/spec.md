## ADDED Requirements

### Requirement: Filesystem selected journal mode
SQLite journal 选择 SHALL 检查数据库父目录，GROW_SQLITE_JOURNAL_MODE 可用大小写无关 wal/truncate 覆盖；空值不覆盖，非法值警告后继续探测。

#### Scenario: 网络文件系统
- **WHEN** 支持的系统探测将父目录判为 network
- **THEN** 选择 TRUNCATE；本地、探测失败和没有实现探测的平台默认 WAL。

#### Scenario: 平台分类
- **WHEN** 运行于 Linux、macOS 或 Windows
- **THEN** 分别使用 statfs magic、MNT_LOCAL/类型名、UNC/remote drive 分类；Linux FUSE 也按 network 处理。

证据：`crates/codegen/sqlite-journal/src/lib.rs` — `for_db_path`；`crates/codegen/sqlite-journal/src/lib.rs` — `is_network_fs`。

### Requirement: Per host rollback database path
TRUNCATE 模式 SHALL 使用基于规范化 hostname 的同目录 .h-<host> 文件名，保留扩展名且重复转换幂等；WAL 不改路径。

#### Scenario: 无有效主机名
- **WHEN** 无法取得可用 hostname
- **THEN** 保留原路径但仍使用 TRUNCATE，不承诺不同主机一定无命名碰撞。

#### Scenario: 重复解析
- **WHEN** 输入已经含当前 host tag
- **THEN** effective_db_path 保持原值，不继续追加标签。

证据：`crates/codegen/sqlite-journal/src/lib.rs` — `effective_db_path`；`crates/codegen/sqlite-journal/src/lib.rs` — `host_discriminator`。

### Requirement: Safe journal conversion and readonly opening
连接打开 SHALL 先配置 5 秒 busy timeout 再设置 journal；TRUNCATE 转换按 EXCLUSIVE → TRUNCATE → NORMAL 顺序执行。

#### Scenario: 只读打开不存在 DB
- **WHEN** 调用 open_readonly 且有效路径不存在
- **THEN** 返回错误，不创建原路径或 per-host DB。

#### Scenario: 网络模式只读查询
- **WHEN** TRUNCATE open_readonly 成功
- **THEN** 使用无 CREATE 的可写 FD 完成转换，然后 query_only 禁止 SQL 写；WAL 用只读 FD。

#### Scenario: 锁转换失败
- **WHEN** 设置 journal 返回 SQLITE_BUSY 或其他错误
- **THEN** 向调用方传播错误，不保证 busy_timeout 能覆盖每一条转换锁路径。

证据：`crates/codegen/sqlite-journal/src/lib.rs` — `open_readonly`；`crates/codegen/sqlite-journal/src/lib.rs` — `apply`。
