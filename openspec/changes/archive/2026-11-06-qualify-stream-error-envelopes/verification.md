# Verification

最终整合实现：SSE入口先确认顶层error成员存在，再对type/object/choices做混合包资格检查。无error帧由原typeddecoder裁决，合法bare/named/typed错误保留共享分类，HTTP非2xx parser本体未改。父预审纠正probe失败回落legacy的重试绕过、显式null成员存在判断、重复helper及过早失败的Messages fixture；最终测试以合法初始帧后遇混合帧证明整次拒收。

实际命令均为Rust1.93.1、--locked --offline，RUST_MIN_STACK=16777216、CARGO_BUILD_JOBS=2。旧red先仅整合新增client测试，保留旧共享分类；编译缺失extra参数修正后再运行，不能将编译失败当red。

| 验证 | 真实结果 | 原始日志 |
| --- | --- | --- |
| 旧分类 `-p sampler --lib mixed_ordinary_error_frame` | 3 run，0 passed / 3 failed，exit101；三个reader均得到Api500/server_error而非Serialization，正中目标反例 | `/tmp/grow-backlog-stream-envelope-old.log` |
| 新分类 `-p sampling-types --lib error::tests -- --test-threads=1` | 42 passed / 0 failed，exit0 | `/tmp/grow-backlog-stream-envelope-green-error.log` |
| 新分类 `-p sampler --lib client::tests -- --test-threads=1` | 75 passed / 0 failed，exit0 | `/tmp/grow-backlog-stream-envelope-green-client.log` |

父亲读原始old错误、running计数与退出记录。新测试覆盖普通身份+error、event名覆盖、null、已声明重复键、未知扩展无error保持原裁决、有error拒收，以及合法bare/named/typed对照。原usage补全的重复字段/数组形状、候选arguments、auth日志等既有client回归保留。

编译适配仅为format!(concat!(...))显式传extra，以及Deserialize签名使用std::result::Result避免本crate别名泛型冲突；生产资格语义不放宽。Responses unknown扩展fixture修复双换行分帧，含error部分使用vendor字段承载扩展对象；相关duplicate/malformed边界由共享分类测试直接覆盖。

独立review已完成：共享资格及三reader定向审查 `01a1af6e-f2b7-7643-8eda-dc2a29f7513a`，控制帧顺序复核 `01a1af86-3663-7303-a824-c419c4f6fafa`。后者的标量check观察被父采纳为必须保留的既有边界，不能用“provider不会发送”替代契约依据；最终probe按JSON对象形状决定是否资格检查，非法JSON、标量、null、数组的knowncheck仍忽略，相关positivecontrol已扩成table。

控制帧旧顺序的真实4项运行见 `/tmp/grow-backlog-check-priority-old.log`：两个mixed帧被吞后报IncompleteStream，2 failed / 2 passed，正中原入口绕过。该实现者使用管道tail导致shell退出码被遮成0，故仅采信原始test输出，不声称取得exit101。修正顺序后 `/tmp/grow-backlog-check-priority-green.log` 4 passed，`/tmp/grow-backlog-check-priority-client.log` 79 passed，green/client命令exit0。该轮尚未包含父随后增加的标量/null/数组对照，主工作树最终复跑仍待记录。合法check的positivecontrol使用collector未启用路径，只证明吞帧后terminal完成，没有断言signals列表。

最终父main复跑包含上述object-shape probe及scalar/null/array控制：`cargo +1.93.1 test --locked --offline -p sampling-types --lib error::tests -- --test-threads=1` **42 passed**，`cargo +1.93.1 test --locked --offline -p sampler --lib client::tests -- --test-threads=1` **79 passed**。两条命令分别捕获真实exit0，组合task也exit0；日志 `/tmp/grow-backlog-stream-final-error.log`、`/tmp/grow-backlog-stream-final-client.log`。父亲核最终reader与完整对象probe，已满足本项验收。

本项未连接真实provider，无Windows现场证据；不声称整个Shell恢复链或重试预算线上成功。Anthropic api_error/timeout_error分类仍为独立后续项。
