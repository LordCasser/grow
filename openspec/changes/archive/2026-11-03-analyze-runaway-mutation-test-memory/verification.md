# 验证记录

- 只读读取 OpenSpec 索引、client-surfaces、sandbox-boundary / tool-authorization 中相关命令与隔离边界；核对已有两个回收修复的归档范围。
- `ps` 核对 PID 3999 的 resume 参数、两次测试进程父链；child summary 核对 parent_session_id。
- `sample <pid> 1 10 -file ...` 保存 PID 27031、32159 的原始报告，没有重跑测试。原始报告保存在本 change，字节和 SHA-256 如下：
  - `sample-27031.txt`：75,823 字节，`529d4917c0f2f215e617d2275384a2f84dfdc91e1c59d4e94cd406fe74f54de9`。
  - `sample-32159.txt`：87,288 字节，`a89855ce093cf613b361cd104ce41cd49f1bfa7338a4fcb934843eb71b43d7b5`。
- 通过系统 libproc 的 `proc_pid_rusage(RUSAGE_INFO_V4)` 独立读取具体 PID 的 footprint / peak；没有只按 RSS 归因。
- canonical 工具开始、完成、tool_result 节选保存在 `tool-evidence.json`；读取时已包含第三次 timeout 和后续恢复记录。
- 核对临时副本环检测、构造环的测试、正常备份与 Solaris 原工作区。没有改动它们；16:18 的只读核对确认环检测已由原 Grow 子 agent 恢复。
- 观察第三次 timeout 后对应测试进程消失、Grow 保持运行和系统 Swap 回落，未主动发送结束信号。
- 完成 `review.md` 中替代归因和失败边界核对。

## 限制

本轮是文档取证，没有 Rust 代码改动，不触发 Rust 编译或新增测试。未启动故障重放或压力测试，没有新增 Cargo 产物。没有实现外部命令内存限制或变异测试 harness 修复；不声称之前缺失的 50G 峰值已被追溯确认。

## 文档校验

- 证据 SHA-256 与字节数核对通过；`tool-evidence.json` 三次 started → timeout 间隔分别为 300.148、300.303、300.106 秒，核对通过。
- Markdown 行尾空白与 `git diff --check -- openspec/backlog.md openspec/changes/analyze-runaway-mutation-test-memory`：通过。
- `openspec validate analyze-runaway-mutation-test-memory --strict --no-interactive`：通过。
- `openspec validate --all --strict --no-interactive`：20/20 通过。
- 16:21:43 的只读末次复查保存于 `post-exit.json`：无对应测试进程，Grow 仍存活，系统 Swap 6,732.69M（约 6.58 GiB）。
- `openspec archive analyze-runaway-mutation-test-memory --skip-specs --yes`：完成，主规范未变。
- 归档后 `openspec validate --all --strict --no-interactive`：19/19 通过。
- 归档后 `openspec validate --archived --no-interactive`：807/807 通过。
- 归档后核对原始采样 hash、节选 JSON 与报告相对链接，backlog 归档链接存在。
