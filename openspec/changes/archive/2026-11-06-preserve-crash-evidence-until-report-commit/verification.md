# Verification

父核对 `check_previous_crash` 新的 `io::Result<Option<CrashReport>>` 契约：missing/empty正常None，其余读错和非空parse失败可观察；有效raw在primary与history分别完成private atomic publication及平台支持barrier前保留。Unix文件创建前0600、文件sync、原子rename、目录sync；新history目录项还须sync其父crash目录。Windows只执行现可移植文件级同步，未声称Unix目录屏障等价。CLI Err跳过O_TRUNC capture安装，保留terminal restore。

初版父审发现Unix Permissions类型路径及新history父目录同步缺口并修正。独立审查 `01a1afa5-4f68-7d80-81f3-d60d1e31a794` 确认提交/错误传播/CLI接线，并发现旧时间戳的本次archive可被同调用的prune删掉。父将其判定为本项承诺缺口，修剪现在排除刚提交路径，在原MAX_HISTORY内删除其他最老记录。父还在测试前核出新excess计数在条件外会下溢，改为仅在超过bound时计算；不引入新storage抽象。

最终主工作树 `cargo +1.93.1 test --locked --offline -p crash-handler --lib -- --test-threads=1` **22 passed / 0 failed、exit0**，日志 `/tmp/grow-backlog-pb07-lib-final.log`。覆盖primary路径阻塞、history阻塞后的部分发布/解除后同identity重试、missing/empty/parse错误、Unixprivate模式，以及旧timestamp配满额较新history时保留当前archive和原bound。之前真实子进程capture套件 **7 passed / 1 ignored / 0 failed、exit0**（ignored仅为子进程entry），日志 `/tmp/grow-backlog-pb07-integration.log`；新prune不改变capture/共享API，未重复该套件。

`cargo +1.93.1 build --locked --offline -p cli --bin grow` 主CLI构建exit0，日志 `/tmp/grow-backlog-pb07-cli-build.log`。父另用该真实CLI执行两个隔离startup controls：合法合成GCRX raw + primary目标目录阻塞→`grow version` exit0、明确恢复/禁capture警告且raw精确字节不变；解除阻塞后相同raw→原version identity报告、primary/history存在，并允许fresh capture创建空raw。结果 `/tmp/grow-backlog-pb07-startup.log`。所有文件在自有临时根，子进程环境隔离，没有修改真实用户证据。

不声称PB-05跨进程raw claim或PB-06跨ASLR符号化已修；这两项仍独立。旧IP解析和history命名格式保持现边界，报告提交失败不会返回不存在路径作为成功。
