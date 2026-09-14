# 测量样本与范围

`session_load_perf` 的 synthetic 分支保留共享 updates/rewind 生成器，并用 Timeline API 生成连续合法的 System head、Turn Started、user/assistant message、Turn Ended；历史轮数随 GROW_PERF_TURNS 变化，assistant 长度随 GROW_PERF_AGENT_CHUNK_LEN 变化。模型显式设为 test/test-model，使用隔离 loopback 配置。生成前对全部参数与保守编码大小执行 256 MiB 上限检查；小默认值为 12 turns。

本轮没有读取或复制真实用户会话，没有运行 dhat/RSS 大样本，也没有新建 worktree/target/profile。128/512 turns 样本均为 cold actor load；各进程独立运行，OS 文件缓存未主动清空，因此不能声称冷磁盘基线。第一条通知包含控制/目录更新，未测 T_first_history 或 T_input_echo p95。

基线参数：8 chunks/turn、4096 bytes/chunk、2 ACU/turn、8 catalog commands、128 bytes description、2 rewind points、2 files/rewind、512 bytes/file。128 turns 重复三次；512 turns 每阶段一个探索样本。样本在进程退出后由 TempDir 清理，构造与 fsync 时间不计入 session/load。

旧 perf fixture 缺少明确模型和合法 Timeline，初次运行失败不能作为产品恢复回归。修正后才采集 before 数据。后续 history 校验的小样本暴露原测试硬编码要求 ACU > 100，与新小默认值不一致；现改为严格检查 load 前最多一条 live catalog，不再用体量代替正确性。

对照结果见 verification.md 与相邻 optimize-session-history-replay 的测量数据。light load/full load 保持同源事件语义，已验证 Timeline 沿 persistence/bootstrap 转移；必要消费者仍持有各自的 surface 或失败恢复副本，不宣称消除所有复制。

A1/A2 的 before 与后续样本共用已实现的用量恢复校验；本组数据比较恢复所有权和解析优化，不能用于推断 A3 自身的性能变化。
