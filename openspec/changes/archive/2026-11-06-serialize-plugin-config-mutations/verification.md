# Verification

主工作树集成已验收。父直接核对 `config/mod.rs` 的现有 SAVE_LOCK 在读前取得、持有到 sibling-temp atomic publication 结束；raw TOML 保留未知字段及环境引用，硬读/语法错误在发布前返回。Shell 安装和 actor/slash/marketplace 调用链 await，Pager CLI await；同步 CTA 点击保留即时 hide/diagnostic，由一个 Effect 在既有 executor 中 await Shell 写入。没有另建配置锁、运行时或持久化队列，没有改 AC-07/AC-08 的 registry 政策。

使用 Rust 1.93.1、`--locked --offline`、`RUST_MIN_STACK=16777216`、`CARGO_BUILD_JOBS=2`，Cargo 窗口串行执行：

- Shell targeted suite：**81 passed / 0 failed、exit0**，原始日志 `logs/tmp/grow-backlog-ac01-shell-final.log`。包含真实设置 `update_config` 与公共 `add_plugin_path` 在隔离 child 的共享锁并发、原始未知键与 `${HOME}` 保留、语法/UTF8/目录读取失败、missing remove、Unix mode 和受阻 atomic publication，以及既有设置持久化与 marketplace install 控制。
- Pager：**3 passed / 0 failed、exit0**，`/tmp/grow-backlog-ac01-pager-final.log`；真实 mouse dismiss → 唯一 persistence Effect → executor 公共 Shell 写入及下一次 CTA recompute。Effect 测试用已有隔离 child GROW_HOME，未写真实用户配置。
- `cargo +1.93.1 check --locked --offline -p cli` **exit0**，`/tmp/grow-backlog-ac01-cli-check.log`；实际 package 为 cli，bin 为 grow，确认非测试 CLI 消费方通过。

初版测试 E0382 把用于后续比较的 Vec move 进 write，改为借用。`plugins = 1` 实际为合法 TOML，独立验证原 add 的目标类型错误、remove no-op 与独立 CTA section 可写，不把它伪称全局语法失败。Pager 新测试最初引用不存在的 `shell::config::grow_home`，已改为真实 `shell::util::grow_home::grow_home` 后重跑通过。没有通过放宽生产读取或丢弃失败场景取得绿灯。

Unix 故障测试在可通过目录 mode 限制的当前非特权环境执行：可读 regular config 的父目录改为 0555，使临时文件发布失败，RAII 恢复权限；失败后原 bytes/mode 保持且无 temp 残留。特权进程若可写目录会明确报告 skip，不能把那种运行记为故障验证。这里不新增跨进程锁或 fsync 保证。

独立 review `01a1afe8-355a-7151-9f8d-7631d7ba9ae8` 直接核实际 source 和两份增量，无 blocking 缺陷；其 CTA 键注释错误已修为 `[plugin_cta].dismissed`，泛型私有 helper 仅为可选简化建议，未扩范围。父核全部原始日志和关键断言后接受特性与 Shell/Pager/CLI 集成；未运行真实远端插件安装、未改用户 config、未提交或发布。
