# Grow 会话外部变异测试内存事故

调查日期：2026-11-03，时间均为 Asia/Shanghai。本轮只做现场取证与文档，没有修改运行代码、终止进程或重跑测试。

## 已证实的原因

会话 `01a19a05-5281-7201-bae3-89f599512353` 的 Grow PID **3999** 启动了子 agent `01a1a066-955a-7050-a9f2-0727eed1456b`（`software-engineering/coder`）。子 agent 在 `/tmp/f9-1-verify` 的隔离 Solaris 副本中进行变异测试，主动把 `src/records/timeline/index.rs:63` 的环检测条件改成 `if false`。

原始 `chain()` 沿 `node.parent` 遍历，每轮向 `Vec<String>` 追加节点 ID。测试 `a_dangling_link_in_a_hand_built_index_is_damage` 在第 176–177 行构造 `n1 → n2 → n1`，第 178 行调用 `chain()`。关闭长度上界后，循环持续 clone 字符串并扩展 Vec，直到进程被结束。两个现场采样均命中这个测试和 `chain()`，其中一个直接命中 `String::clone` / `_platform_memmove`。

这次数十 GB 增长属于 **Grow 启动的测试子进程**，不是 Grow PID 3999 的同量 heap 增长。它与此前确认的前端 transcript 保留、retiring 容量漏算是独立路径；这不撤销前两项缺陷，也不能反推重启前未被采到的 50G 一定具有相同原因。

## 进程和内存证据

| 时间 | 观测 |
| --- | --- |
| 16:06:51 | 测试 PID 27031 RSS 6,580,240 KiB，祖先链为 `27031 → 26825(cargo) → 26824(bash) → 26783(bash) → 3999(grow)`。 |
| 16:08:53.361 | `sample` 报告 PID 27031 的 footprint 和 peak 均为 **97.0G**；线程为上述环测试，栈命中 `chain → Option<String>::clone → String::clone → memmove`。 |
| 16:13:52 | 新测试 PID 32159 的 `proc_pid_rusage` footprint 为 **58.85 GiB**；父链 `32159 → 32136(cargo) → 32130(bash) → 3999(grow)`；Bash 命令再次明确注入 `if false`。 |
| 16:14:46.412 | `sample` 报告 PID 32159 的 footprint 和 peak 均为 **104.8G**，同一个环测试的栈仍在 `chain()`。 |
| 16:16:19 | 系统 `vm.swapusage` 为 `used = 80913.88M`，约 **79.0 GiB**；测试 PID 32159 仍存活。 |
| 16:16:42.270 | canonical 工具结果为 `exit: killed (timeout)`；随后该测试进程及其 cargo/Bash 父进程消失。不是本轮调查主动终止。 |
| 16:17:50 | 系统 `vm.swapusage` 为 `used = 7308.69M`，约 **7.14 GiB**；Grow PID 3999 仍存活。 |
| 16:18:28 | Grow 的 `proc_pid_rusage` footprint **1.981 GiB**，生命周期 peak **3.029 GiB**；对应近期 jemalloc trace 活跃分配约 **1.03 GiB**。未发现该测试二进制仍在运行。 |

用户两张系统面板截图分别显示 Swap 83.58 GB、68.69 GB，进程列表截图明确显示 `solaris_agent-ce7d649adce8d8fb` 占用 75.93 GB。它们与本次现场相符，但时间、单位、内存口径不同，不把各张截图与某个非同时的内核读数强行对应。RSS 会因压缩、换出而显著小于 footprint，不能只按 RSS 排名排除该测试进程。

原始采样见 [PID 27031](sample-27031.txt)、[PID 32159](sample-32159.txt)；canonical 工具记录节选见 [tool-evidence.json](tool-evidence.json)。

16:21:43 的末次只读复查仍无对应测试进程，系统 Swap 约 6.58 GiB，Grow footprint 约 2.59 GiB；原始数值保存在 [post-exit.json](post-exit.json)。

## 为什么重复出现

1. 16:01:21，子 agent 启动 mutation battery，请求 600,000 ms；16:06:21 实际收到 `exit: killed (timeout)`。输出只包含 M1–M8 的 `KILLED`。M9 关闭环检测后未完成，工具超时结束了 shell，脚本写在 `cargo test` 之后的普通 `cp` 没有执行。
2. 16:06:26，子 agent 将其解释为剩余变异需要更长预算；16:06:27 请求 420,000 ms 再运行 M9/M10。M9 输出 `NOPAT` / `SKIP`，说明前次关闭的条件尚未恢复；脚本接着运行 M10，并带着 M9 的变异状态再次执行整组 timeline 测试。这次的 PID 27031 被采到 97.0G。16:11:27 工具再次以 timeout 结束，约 300 秒。
3. 16:11:38，子 agent 显式恢复环检测；16:11:42 又单独关闭它重跑 M9，产生 PID 32159。16:16:42 再次在约 300 秒后 timeout，期间采到 104.8G。
4. 16:16:46，子 agent 的后续工具记录显示 `if false` 后恢复为 `if path.len() > index.nodes.len()`，输出 `RESTORED`。16:18 的只读文件核对确认临时副本、`/tmp/b3.rs` 和原 Solaris 工作区均存在正常长度上界。

脚本没有异常退出清理，超时恢复点在被结束的 shell 内部。第一轮残留变异还污染了下一轮 M10，不能把那轮结果当作只测试 M10。第三轮则是显式恢复后再次主动注入，不能将所有重复增长都解释为残留代码。

已有工具超时确实结束了进程树，当前未见该测试的孤儿进程；问题是五分钟内足以增长到百 GB 量级。模型请求更长 timeout 没有改变本运行实例约五分钟的实际结果，也没有解决无界分配。

## 结论边界与后续事项

- 这不是虚拟地址空间误读：证据为具体测试 PID 的 footprint，而不是巨大的未提交 Guard 映射；栈和主动注入的循环相互印证，退出后系统 Swap 同时大幅回落。
- 同期其他 rustc 等进程也在使用内存，不能把全部系统压力字节归给这个测试；但上述测试已独立达到 97.0G / 104.8G，是本轮明确的大头。
- Solaris 原工作区存在正常环检测；缺陷发生于变异测试的执行和清理方式，不能直接修改正常算法来“修复”变异体。
- timeout、资源中断、测试断言失败和编译失败需要分别解释；本次 M9 的超时不是回归测试通过，也不能自动算成可靠的 mutation `KILLED` 证明。
- 此前两项 Grow 源码修复尚未进入 12:33 启动的 PID 3999。即使部署，它们也不会给外部 `cargo test` 增加内存边界；本次不宣称已修复该第三条路径。
- 外部高风险测试的资源预算、临时变异的可靠恢复及中断结果解释，应独立确定修复边界，见 backlog。没有为取证再次启动失控工作负载。
