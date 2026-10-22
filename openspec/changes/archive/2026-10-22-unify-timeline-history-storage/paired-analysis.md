# 最终 release 配对结果

使用最终 production CLI4 与 build21 performance executable；其后只有纯 rustfmt、cfg(test) reader 与工具 integration test 变化，见 `final-source-audit.json`。早期 root 目录 CLI3 数据保留于 [intermediate-paired-analysis](intermediate-paired-analysis.md)，不计入本表。所有以下原始文件均在 [final-paired](final-paired/formal-run-context.json)。

Rust 1.93.1，普通 release，三对 AB/BA，冷进程/温热页缓存；fixture 不进入 RPC；测量与本任务编译/正确性运行串行，外部主机负载不受控制。以下为 median [min, max] ms；n=3 不能作为分布保证。

| Profile | Load RPC before → after | 首条历史 before → after | 末条历史 before → after | 最大通知间隔 before → after |
| --- | ---: | ---: | ---: | ---: |
| modern-128 | 110.421 [91.448, 120.378] → 107.577 [96.972, 109.135] | 38.143 [36.983, 42.981] → 21.624 [20.350, 23.511] | 41.813 [40.176, 46.640] → 24.748 [23.567, 26.422] | 1.117 [1.066, 1.266] → 1.303 [1.142, 1.346] |
| modern-512 | 204.741 [202.990, 227.620] → 163.041 [154.806, 167.480] | 113.586 [110.969, 113.671] → 62.597 [60.390, 63.036] | 149.518 [146.326, 154.883] → 94.608 [91.812, 95.534] | 13.191 [12.798, 17.576] → 9.236 [8.608, 9.647] |
| modern-2048 | 747.114 [651.795, 773.142] → 416.701 [386.386, 433.577] | 438.256 [417.970, 470.159] → 232.331 [223.261, 246.339] | 648.526 [586.939, 670.444] → 386.457 [363.684, 406.163] | 64.097 [51.445, 65.345] → 25.244 [18.773, 26.074] |
| modern-repair-512 | 471.165 [389.719, 544.510] → 158.764 [149.062, 163.980] | 371.313 [299.264, 443.625] → 62.241 [60.489, 63.282] | 407.883 [334.265, 477.757] → 96.072 [92.504, 100.355] | 13.614 [12.776, 14.110] → 10.465 [10.060, 11.752] |
| modern-short-512 | 126.170 [120.895, 172.291] → 117.835 [114.125, 146.784] | 41.612 [41.257, 55.307] → 29.071 [29.006, 35.733] | 57.420 [56.356, 76.171] → 41.471 [40.889, 64.119] | 5.823 [5.540, 9.320] → 1.811 [1.747, 5.514] |

2048 轮 RPC 中位数下降 44.2%，首条历史提前 47.0%。128 轮总耗时改善很小，不能声称所有规模具有相同收益。首/末条历史是 ACP 接收时间，不是终端实际绘制。

| Profile | 完整样本目录 bytes before → after | 文件数 before → after |
| --- | ---: | ---: |
| modern-128 | 9,263,748 → 4,679,257 | 5 → 4 |
| modern-512 | 37,042,333 → 18,706,802 | 5 → 4 |
| modern-2048 | 148,178,363 → 74,849,404 | 5 → 4 |
| modern-repair-512 | 35,980,484 → 18,706,802 | 5 → 4 |
| modern-short-512 | 7,682,205 → 4,026,738 | 5 → 4 |

2048 轮目录由 148,178,363 降至 74,849,404 bytes（−49.5%）。Timeline 自身从 71,032,018 增至 74,843,515 bytes，旧 updates 从 77,140,456 变为 0。减少的是重复正文及第二文件，不是删除历史；实际包含大量 artifacts 的真实目录不保证相同比例。

## 正常 durable 提交

每进程 3 warmup + 21 response rounds，三对进程；以下是各进程 median 的 median/range，p95 列也是各进程 p95 的 median。窗口为 candidate anchor + admission +旧 projection build/commit，不是整个 turn。

| Payload | Stage | Total median [range] ms | p95 median ms | Ledger barriers/response |
| --- | --- | ---: | ---: | ---: |
| 4 KiB | before | 33.233 [29.640, 43.738] | 43.256 | 6 |
| 4 KiB | after | 16.238 [14.513, 18.118] | 22.256 | 4 |
| 32 KiB | before | 26.918 [24.204, 49.474] | 38.618 | 6 |
| 32 KiB | after | 14.131 [13.717, 18.222] | 22.567 | 4 |

减少两个 barrier 来自第二 projection 提交的移除；Timeline 的 file/directory durable ACK 保留。计数只覆盖 Timeline/updates 两类 ledger，排除 summary/artifact/其他 syscall；正常写入成本没有转移成恢复时的第二正文修复。

## 真实终端

三对 modern PTY；history visible 是最终 sentinel 可见，ready 是 SessionLoaded/replay end。

| Turns | 指标 | Before median [range] ms | After median [range] ms |
| --- | --- | ---: | ---: |
| 128 | history_visible_ms | 163.9 [160.5, 191.2] | 164.2 [163.9, 178.4] |
| 128 | ready_ms | 242.4 [215.0, 244.8] | 218.6 [193.7, 230.5] |
| 128 | ready_echo_p95_ms | 5.8 [4.2, 6.3] | 5.2 [4.2, 6.4] |
| 512 | history_visible_ms | 428.5 [428.4, 451.0] | 446.2 [446.1, 451.2] |
| 512 | ready_ms | 511.4 [509.1, 560.4] | 472.3 [472.1, 481.4] |
| 512 | ready_echo_p95_ms | 5.8 [4.3, 6.0] | 4.3 [4.3, 6.1] |

PTY ready 更早，但末屏没有一致改善：128 轮近似持平，512 轮末条可见中位数从 428.5 增至 446.2 ms。单对 slow-writer 的 first 为 291.9 → 321.5 ms、last 为 535.4 → 582.0 ms、最大可见间隔 136.9 → 154.9 ms，均 3 次可见推进；这项回退必须保留，不能被 RPC 改善掩盖。它证明当前有界持续进度和退出行为正确，不证明慢终端更快。加载中 Ctrl+D 的 before/after 均实际通过。

## 内存与剩余成本

完整进程 peak RSS（包括 fixture/后置读取）为 768196608.000 [723648512.000, 771489792.000] → 749174784.000 [699367424.000, 859389952.000] bytes，范围重叠。
粗 `ps` before：fixture-complete 至退出 peak median 738.5 MiB [732.9, 744.2]，首样本 430.3 MiB。
粗 `ps` after：fixture-complete 至退出 peak median 708.9 MiB [644.2, 773.5]，首样本 501.1 MiB。

粗采样包含 actor 初始化/清理和 observer 成本，可能漏掉短峰值；短正文样本 RSS 也略升，旧候选批次方向并不一致。因此不承诺常规 resume 内存下降。单独 fork 300 项的原 `<2x` 流式分配门槛通过 1.714116x，是不同范围的有界性证明。

按 RPC 中位数选出的 after 2048-run：journal read/decode 201.7 ms（嵌套在 load_light 210.7 ms 内），history projection 11.6 ms，replay interval 74.4 ms，actor spawn/register 90.0 ms，完整 load interval 415.8 ms，首条历史 232.3 ms。before 对应 run：Timeline decode 84.0 ms、updates decode 73.8 ms、projection plan 141.8 ms（内含 canonical 136.5 ms）、replay 216.1 ms，首条历史 470.2 ms。区间嵌套且可能重叠，不能相加。统一 decoder 因新校验及完整 typed facts 变重，但第二文件解码、第二投影和重读成本消失，总体下降；当前首条历史前的主成本仍是完整 journal 解码与校验，复杂度依旧线性。

repair512 的 before 会刻意缺每 16 条 projection；after 没有第二 projection 可修复，目录同 ordinary512。这一行包含结构性工作量差异，不能解释为等工作量 repair 加速。

证据：`final-paired/paired-summary.json`、两组 measurements/timelines、commit/PTY/RSS JSON 与全部逐次 trace/text；二进制 SHA 和实际命令见 `final-paired/formal-run-context.json`。
