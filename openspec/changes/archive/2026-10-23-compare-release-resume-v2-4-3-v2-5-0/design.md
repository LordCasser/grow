# 对照方法

使用两个 tag 的官方 release-dist 二进制，核对 SHA、attestation/source、codesign 与版本；不将普通 release 或 debug 混入。固定同机 macOS ARM64、50×120 PTY、诊断关闭、每轮32KiB assistant正文及同样 user/catalog/rewind 配置。现有 synth 写合法现代 admission/history，再仅对2.4.3用既有 test-only exporter 写它支持的 Timeline+updates/summary v6。当前 journal/summary v7 给2.5.0。核对逻辑正文数量/字节/摘要，版本各自仍运行生产完整恢复，不在产品内加兼容路径。

每次独立 HOME/GROW_HOME、cwd和进程，本地 mock、不使用个人配置或付费 provider。夹具生成与正确性摘要在计时外；生成后页缓存温热，不宣称冷盘。正式测量与编译串行，预热独立排除，最终512/2048/4096各五对（6144因干扰未形成足够正式样本），按AB/BA交替。记录每次值、median/min/max及版本身份，仅最终结论入仓，原始cast/日志留外部并收尾清理。

起点为调用PTY spawn前。首条历史为实际模拟屏幕首次出现已知user/assistant正文，不要求一定显示turn1；末屏为最后一轮唯一sequence marker首次可见。可输入时间需在末屏且Loading消失后发送不提交的唯一草稿探针并观察回显，不以模型标签、end_batch或最后正文代替输入启用；这是外部可观测全历史后输入回显上界。另断言恢复/探针不触发推理，Ctrl+D正常退出。窗口缺失、旧格式拒绝或内容不等价则停止，不把失败算性能样本。

不进行就绪与历史分页分离。小样本与混合媒体不在本次文本长会话的收益承诺内。专用Cargo target、下载和隔离会话在验证后删除，保留既有本机官方候选，不清共享目录。

用户追加更长样本：4096/6144轮约128/192 MiB assistant正文，保持现有256 MiB synth预算，不放宽生产或夹具限制。原512/2048矩阵保留；新增仅允许配置轮数，实际观测/输入探针代码不变，两组各自核对source和executable身份，仍使用同两份官方CLI。

## 手动复测入口

编译观察器：固定Rust 1.93.1，独立 `CARGO_TARGET_DIR`，执行 `cargo test --locked --release -p pager-pty-harness --test release_resume_comparison --no-run`。目标为已经核验来源的两个官方release-dist executable，并非该target编译的Grow。

执行该test executable的 `--ignored --exact official_release_resume_comparison --nocapture --test-threads=1`，提供 `GROW_COMPARE_OLD_BINARY`、`GROW_COMPARE_NEW_BINARY`、已创建的空 `GROW_COMPARE_OUTPUT` 和隔离 `HOME/GROW_HOME`。默认512/2048轮，`GROW_COMPARE_TURNS=4096,6144`扩展；`GROW_COMPARE_SMOKE=1`仅做排除的512轮预热。样本输出JSONL和cast，仅保留最终聚合到change。

每个规模的pair0排除且进行完整wire oracle，pair1..5交替BA/AB。任何格式错误、超时或身份/顺序不等立即失败，不能记作快样本。编译与测量串行，另外在外部脚本等待60秒编译安静窗口，测量期间每0.5秒监测编译进程；命中则整轮排除。测试自身不会终止其他任务的编译。


## 扩展矩阵的编译污染处理

两轮4096/6144完整矩阵均因外部编译而整体排除。后续测量在启动前固定改为完整pair级排除，旧失败矩阵不回收任何样本。`GROW_COMPARE_PAIR=0..5`只选择驱动范围，观测代码不变。每规模先pair0排除预热并做两版wire oracle，随后pair1..5保持原BA/AB次序；每对独立observer进程，夹具/子进程仍每样本新建。首对前等待10秒编译安静窗口（此协议尚未开始采样前确定），每对前短暂确认无编译；受污染整对重测同pair index，不基于耗时选择。预热始终不计时统计；正式每对均需0监测命中，source/binary前后SHA一致。每对最多20次尝试，未取得5对则不发布该规模正式收益。
