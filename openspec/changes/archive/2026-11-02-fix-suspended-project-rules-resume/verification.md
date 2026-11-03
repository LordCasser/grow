## 本机证据

目标 session 位于用户 Grow home 的 Solaris Agent 项目目录。失败日志为 `~/.grow/logs/unified.jsonl` 两次 `session.load.failed`；时间为 2026-11-02 18:05:14.642 和 18:05:33.740（Asia/Shanghai）。错误均为 `original workspace, Agent source or model route changed; tree remains suspended`。

临时诊断仅在 test 构建中启用，真实 load 在 source 校验处强制退出，不启动 provider、不放行原 Turn。Rust 1.93.1，CARGO_INCREMENTAL=0、dev/test debug=0。首次探针 1 passed，隔离 session 修复后再运行同一产物 1 passed；第二次打印 current == expected == 新 source hash。两次均通过前置 JSONL、Timeline、artifact 与 actor 重建。诊断编译期间修正过 EventSeq 的日志格式错误；最终两次运行均通过。临时代码和私有候选数据已移除，不进入产品构建。

## 实施

备份：`~/.grow/recovery/2026-11-02-01a19a05-5281-7201-bae3-89f599512353-resume-source/timeline.jsonl`，同目录 `repair.json` 保存身份、旧/新 hash 和前后 SHA-256。

仅 physical record 9254（domain event 7060）的 checkpoint source hash 与 checksum 改变；原其他记录完全相同。校验和采用仓库原有 `grow.timeline.record\0` + record version + physical sequence LE + 原 fact 字节的 BLAKE3 格式。修复前原 checksum 已独立核对。

## 限制与清理

不声称实际续跑成功；为避免替用户重新执行工作，探针在准备恢复前停止。用户可直接使用原安装版 grow resume；没有改安装二进制或目标项目文件。临时 session 副本与 source candidates 清理，不清理其他任务共享的 Cargo 产物。

## 规范校验

归档前全量 strict 18/18 通过；归档后全量 strict 17/17、archive strict 764/764 通过。`git diff --check` 通过。产品代码无保留修改。

## 用户重试后的实际确认

修复后用户侧新进程 pid 86472 于 2026-11-02 18:15:22.935 开始加载，18:15:28.286 记录 `session.load.done`。Timeline 新增 domain events 7062 RestorePrepared、7063 Resumed、7064 ExecutionReleased；`shell.turn.inference_start` 确认原任务已继续。上述执行由用户恢复触发，本代理未主动启动原任务。最终文件 hash 已因这些合法新事实改变，不能将其当作修复失败或恢复旧备份覆盖。
