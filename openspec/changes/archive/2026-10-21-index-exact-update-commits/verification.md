# 验证记录

> 归档整理（2026-10-22）：仅保留本报告中的最终验证结论、测量口径与限制。逐次运行数据和测试输出已从仓库清理。

2026-10-21，本地 macOS；基线 HEAD 为 a1d1c2965075c87645eab22adce6d48b19410c59，优化结果包含当前工作树改动。所有数据均为合成历史、dev/test 未优化构建，不能当作 release 或真实用户会话的速度承诺。

构建统一使用 CARGO_INCREMENTAL=0、CARGO_PROFILE_DEV_DEBUG=0、CARGO_PROFILE_TEST_DEBUG=0、CARGO_BUILD_JOBS=2：

```sh
cargo test --locked --offline -p chat-state -p shell -p pager --features shell/test-support --lib --test session_load_perf --test pty_e2e_persistence --no-run
```

先通过已编译产物的 --exact --list 确认每个 ignored 基准恰好一项，再以 RUST_MIN_STACK=16777216、--exact --ignored --nocapture --test-threads=1 串行运行三次。fixture 建立与正确性断言在计时窗口外，前后规模、输入及构建参数一致。性能测量时没有并行启动本任务的编译或测试；共享机器其他负载仍会带来噪声。表格为中位数（最小–最大），原始日志保留全部三轮结果。

## 测量

128/512/2048 条 projection，每条正文 4 KiB，物理 updates 分别约 0.57/2.26/9.06 MB。每组先作 16 次 exact lookup，再作 16 次新的 durable exact commit。优化后的首次 lookup 包含冷索引构建，commit 包含实际追加、文件同步与目录同步。

### 16 次 lookup

| 规模 | 优化前（ms） | 优化后（ms） | 前/后 |
| --- | ---: | ---: | ---: |
| 128 | 51.887（51.435–56.160） | 4.122（4.107–4.806） | 12.59× |
| 512 | 205.529（204.049–209.639） | 13.464（13.364–14.182） | 15.27× |
| 2048 | 824.992（811.066–852.283） | 51.692（51.498–52.012） | 15.96× |

### 16 次 durable commit

| 规模 | 优化前（ms） | 优化后（ms） | 前/后 |
| --- | ---: | ---: | ---: |
| 128 | 268.538（262.745–321.592） | 175.881（149.601–190.810） | 1.53× |
| 512 | 601.935（601.297–620.488） | 173.721（153.367–195.602） | 3.46× |
| 2048 | 1820.747（1785.907–1905.056） | 162.541（157.321–181.227） | 11.20× |

首次索引仍是 O(物理日志字节数)；后续按身份查找对应物理位置，仅核对命中的完整 payload。普通 owner append 增量维护索引。表格不代表单次提交延迟，也没有去掉同步成本。

## 正确性与对抗性复核

JSONL 模块 179 passed、0 failed、1 ignored，持久化模块 82 passed，分别见 JSONL 和 persistence。包括 ACP/projection exact、重复与冲突、失败后的可读记录、file/directory sync 失败重试、不重复追加、session load、rewind、fork 和 cursor 边界。

新回归覆盖普通更新与 exact 混排、同一身份的全部重复记录（任一 payload 不同即 Conflict）、同长度外部修改、物理文件替换及无换行尾部被后续 append 终结。缓存不保留正文或文件句柄。Unix 使用 dev/ino，Windows 使用 volume/file index 识别实体；文件 stamp、大小和实体变化均重建。Windows 分支与项目既有句柄身份 API 一致，但本次运行平台为 macOS，未宣称 Windows runtime 验证。

索引推进的前后 metadata 在同一 append advisory lock、同一已打开文件内取得；只认可精确长度增长与完整已提交前缀。写入/flush/sync 任何失败、未知变化或 torn tail 均丢弃缓存，之后重新读取。索引命中只证明物理记录 exact，不能代替 durable ACK；原 file + directory 同步和 retry barrier 保留。writer epoch/lease 释放时清除缓存。

## 原始数据

优化前、优化后。

## 归档校验

四项 change 各自完成 tasks 后独立归档；只有加载交互修改 client-surfaces 主规范，三项内部优化使用 skip_specs。归档前全量 strict 为 20 passed；归档后全量 strict 为 16 passed、archive strict 为 671 passed，均 0 failed。修改 Rust 文件的 rustfmt 检查、git diff --check 和本组文档链接检查通过。原始 stdout/stderr 使用 .txt 保存，避免被仓库的 *.log 忽略规则排除。
