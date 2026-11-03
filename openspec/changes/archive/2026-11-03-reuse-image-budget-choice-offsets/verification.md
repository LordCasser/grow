# 验证

2026-11-03，临时独立GROW_HOME、RUST_MIN_STACK=16777216、Cargo --locked。

1. 抽出原inline validation为纯helper，原SID-key算法保持；`cargo test --locked -p chat-state --lib image_budget_choice -- --include-ignored --nocapture --test-threads=1` 旧producer基线2 passed。初始fixture误将User默认Text part0当图片，修改为显式image-only setup后再采集，不改oracle/生产/断言。
2. offset表示后相同命令2 passed；旧SID线性oracle与当前helper全serialized item差分，覆盖零/单/多source、多part、越界/非Image、新SID同位置、新URL Arc同文本、新description Arc同文本、prefix append，builder全清/pressure逐项skip；非单调SID验证有效parts报告顺序。
3. 移除临时probe后 `cargo test --locked -p chat-state --lib -- --test-threads=4`：603 passed、0 failed、3 ignored，17.73秒。三项为已有冻结replay/online append/admission手动bench。包含真实build_request的sticky预算、append/repeat/native reset/new高水位、工具图片不改Timeline，以及context-pressure route切换和其他Timeline/ACK回归。

定位validation+apply样本，N=8192，单位ns（before/after各一个独立warm进程，2 warmups+7 samples）：

| 位置与选择 | 旧actual中位 | 新actual中位 |
| --- | --- | --- |
| front D0/K0 | 42 | 42 |
| front D1/K1 | 375 | 375 |
| tail D1/K1 | 46500 | 375 |
| front D2/K2 | 541 | 584 |
| tail D2/K2 | 91375 | 541 |
| front D64/K64 | 26667 | 13375 |
| tail D64/K64 | 3133042 | 13000 |
| front D1/K16 | 3208 | 3458 |
| tail D1/K16 | 792333 | 3417 |

完整原始样本、fixture与临时probe见[measurement](measurement.md)。前部少量选择可能持平/有几十至数百ns波动；不宣称所有负载单调提速，不为此引入模式阈值。长text前缀后选择不再随N重复线性扫描。只测小图内核，setup/clone与effective-parts报告不计时；没有声称整request构建、provider、编码、吞吐或RSS收益。每个进程另保留legacy Vec oracle控制，其迭代常数不等于原BTreeMap，不混淆成同一实际数据结构。

独立review最终通过；rustfmt和git diff --check通过。没有新索引、cache字段、迁移或持久格式变更；source/Weak校验与域重置不变，skip_specs。一次实现编译错误是Option.and_then返回slice的类型不匹配，改成if-let逐级访问后执行矩阵通过，无业务语义改动。

归档前 `openspec validate --all --strict --no-interactive` 17/17通过；归档后同命令17/17、`openspec validate --archived --strict --no-interactive` 790/790通过。
