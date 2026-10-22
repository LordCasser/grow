# Verification

> 归档整理（2026-10-22）：仅保留本报告中的最终验证结论、测量口径与限制。以下提到的逐次运行数据、构建日志、诊断快照和中间结果曾用于验证，现已从仓库清理；历史“保留原始记录”表述不再表示文件仍在仓库中。

固定 Rust 1.93.1、普通 release、`target/resume-release`、jobs=4、incremental=0。优化前后构建和正式测量均使用同一工具链及配置。完整逻辑历史夹具固定为 148178363 bytes、5 个文件。逐次运行记录与构建身份快照已清理，最终结论见本报告。工作树同时包含其他并行改动，其 diff 指纹仅用于识别快照，不表示所有 dirty 文件都属于本 change。

## 正确性

- Shell 相关回归共 522 passed；Pager 加载/输入/退出/resize 与 reconnect 共 29 passed；ChatState 共 571 passed（Timeline 138，其余 433），默认 ignored 3 个重型项。
- 优化前后四个完整历史 oracle 均通过。现代 CLI 的 128/512-turn 真实 PTY 各运行 3 个进程，另验证 512-turn 慢输出以及加载中 Ctrl+D 退出后再次恢复，均通过。
- malformed/version/size/torn-tail、projection exact/conflict、repair、rewind/fork、cursor/resident、typed/raw projection 顺序和重复 typed 字段拒绝等回归通过。loading 时普通输入仍忽略，Ctrl+D 和 resize 可用。
- 重复 typed payload 字段是唯一明确的 wire 拒绝边界 delta：旧 `Value` 中转会覆盖重复键，直接 typed 解码现在拒绝歧义输入。此项属于契约变化，已写入 session-timeline delta；本 change 不再标记 `skip_specs`。

## 性能与资源

五组 profile 各测 3 个独立进程。modern-2048 的 load RPC 中位数从 943.225 ms 降至 765.529 ms，first-history 从 550.976 ms 降至 424.361 ms。对应阶段中位数：Timeline read/decode 110.395→67.418 ms，forward updates 63.406→22.917 ms，typed canonical 138.428→136.741 ms。优化后 writer_prefix 为 84.707 ms，列为下一项独立优化目标。

modern-repair-512 load RPC 从 489.175 ms 升至 537.385 ms；后续重复复测仍慢，不能据此认定为测量噪声，也不能把回归全部归因于噪声。复测差异集中于未修改的 durable append 阶段；双方的独立 sample 均显示 `sync_file_durable` 的 `fcntl` 同步等待。此证据不能解释所有 wall time 差异，不宣称 repair 改善；第三项移除冗余 cache repair 后重新测量，不降低同步等级。

真实 PTY 的普通 512-turn history-visible 中位数为 552.9→690.8 ms（优化后较慢），因此不宣称 PTY 变快；128-turn 小样本中位数为 270.8→196.0 ms。慢输出场景 512-turn 为 671.7→576.3 ms。PTY wall time 受主机调度影响，以上数值只代表该次本机测量。

正式整进程 peak RSS 中位数为 682573824→771850240 bytes；计入夹具生成及后置 updates 全量读取，不是 isolated-load 峰值。额外粗采样未显示一致增加，但采样不能排除短时峰值，不能宣称内存下降。夹具逻辑大小和文件数前后不变。

## 结论与待办

常见现代恢复路径及首屏历史在正式 profile 中改善；repair 场景存在待处理回归，真实 512-turn PTY 未改善。收益和限制均已保留，未将父阶段总耗时视作可删除工作。实现与测量任务已完成。格式与 git diff --check 通过；全量 OpenSpec strict 20 项通过。逐次验证输出和性能比较文件已清理；上述结果与限制为最终归档结论。独占 target/resume-release 依赖供后两项复用，全部结束后仅清理该目录，不动原有 target 缓存。
