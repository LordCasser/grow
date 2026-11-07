# PB-04 runner cleanup verification

状态：父已核实际diff、oldred/green日志、fixture退役和错误语义，接受本独立修复。开发说明已同步；父归档前全量strict为23 passed/0 failed，actual exit0（`/tmp/grow-pb04-parent-strict.log`）。归档后门禁已完成：`openspec archive report-detached-runner-cleanup --yes` actual exit0，合入development-workflow一项MODIFIED requirement并归档为`2026-11-07-report-detached-runner-cleanup`（`/tmp/grow-pb04-parent-archive.log`）；全量strict为22 passed/0 failed（`/tmp/grow-pb04-postarchive-strict.log`），archived strict为904 passed/0 failed（`/tmp/grow-pb04-postarchive-archived.log`），均actual exit0。首次误用不存在的`--archives`选项exit1，日志保留，改为仓库规定的`--archived`后成功；该参数错误不计规范失败或通过证据。只覆盖本 change 的 Unix exact runner 确认修复，不代表 PB-03、Rust/PTY 产品或父任务整体完成。

## 实现与不变量

- `scripts/run_exact_cargo_test.py`：保留 Unix 的组 TERM/KILL、直接子进程有界 wait/kill fallback。Unix 强制清理返回未确认；组信号成功、直接 child 已 reap 和 reader EOF 不再合成为完整 tree confirmed。timeout、KeyboardInterrupt/SIGTERM 取消与 finally 均使用该结果，finally 的布尔合取不能把未确认覆盖为 true。
- Unix 未确认诊断明确为 `process-group cleanup cannot confirm detached descendants stopped`。原 timeout 错误、取消异常及 main 的退出码保持。未执行强制清理的正常 phase 保留原完成条件。
- Windows taskkill 分支、deadline、单 Cargo 编译产物、三阶段、exact/matching 检查未改；无新增依赖、全局进程枚举、持久化 PID 表或进程管理框架。
- `scripts/tests/test_run_exact_cargo_test.py`：复用原 SIGTERM 用例的真实 main wrapper；新增一个共用 socket 夹具的回归，含 timeout、真实 SIGINT 和真实 SIGTERM 三个子场景。

## 真实 old red

在修改 runner 前执行：

```sh
python3 -m unittest scripts.tests.test_run_exact_cargo_test.RealCommandTests.test_detached_devnull_descendant_cleanup_is_unconfirmed -v
```

实际 exit 1，`Ran 1 test in 2.209s`，3 个子场景均在 `cleanup unconfirmed` 断言失败，非编译/夹具失败。原始失败日志保留在 `/tmp/grow-pb04-oldred-01a1b1db.log`（终端任务 `01a1b1de-c483-7180-b217-a4f6b21c4f16`，已终态）。

三个子场景在失败前均已证明：真实 setsid＋DEVNULL 后代完成私有握手并回应 ping；runner 结束后直接 phase child 的 PID 已不存在，而 detached 后代仍回应 ping；runner 自身退出码分别为 timeout=1、SIGINT=130、SIGTERM=143。

旧 timeout 只有 `Cargo execute timed out after 2s`，未附 cleanup unconfirmed；旧 SIGINT/SIGTERM 则输出 `cancelled; process tree termination confirmed`。原始失败没有 teardown failure，逃逸夹具在 finally 通过私有 stop/EOF 退役。随后自查收紧最终夹具为关闭私有连接写端并核 EOF，断言与反例结构保持。

## 修复后验证

环境：Darwin，Python 3.9.6。以下命令均未调用 Cargo，后台命令使用 timeout=0，均已收取 actual exit：

```sh
python3 -m unittest scripts.tests.test_run_exact_cargo_test.ExactCargoTestTests scripts.tests.test_run_exact_cargo_test.RealCommandTests -v
```

实际 exit 0，`Ran 18 tests in 12.299s`，无 skip/failure。日志：`/tmp/grow-pb04-noncargo-01a1b1db.log`（任务 `01a1b1e0-0fc2-76b0-9bab-55982cf5c7aa`，已终态）。覆盖原三阶段/单产物/exact/ignored/错误输出断言；真实正常流式 phase exit 0；真实同组后代 timeout、KeyboardInterrupt、SIGTERM 清理；setsid 后代持 pipe 的有界未确认；读错误；新增 setsid＋DEVNULL 三种中断反例；Windows taskkill mock 接线。

最终夹具移除按握手 PID 发信号的后备清理后，只重跑受影响回归：

```sh
python3 -m unittest scripts.tests.test_run_exact_cargo_test.RealCommandTests.test_detached_devnull_descendant_cleanup_is_unconfirmed -v
```

实际 exit 0，`Ran 1 test in 2.206s`，三个子场景均通过。日志：`/tmp/grow-pb04-owned-fixture-01a1b1db.log`（任务 `01a1b1e1-9371-7a21-ba46-cc415abffc7c`，已终态）。每个场景检查 wrapper 的实际退出码 1/130/143、直接 child 已 reap、detached 后代在 runner 返回后仍存活、诊断包含 cleanup unconfirmed 和 detached 且不含 termination confirmed；cleanup 返回在 5 秒断言内。

最终 fixture 只在自己的短路径临时目录监听私有 Unix socket。runner 的真实 child 创建 setsid 且 stdin/stdout/stderr 全为 DEVNULL 的后代，该后代独占连接。PIDs 仅来自本次握手，用于 session/group 与直接 child 退役观察，不是信号权限来源。每个 finally 对活连接执行 `shutdown(SHUT_WR)`；fixture 收到 EOF 立即 `os._exit(0)`，测试在 3 秒 socket bound 内读到返回 EOF，确认退出。随后关闭连接并 communicate/reap 自己的 wrapper。该证明不依赖孤儿进程何时被系统 reap。没有遗留持续进程或 watch。

`git diff --check -- scripts/run_exact_cargo_test.py scripts/tests/test_run_exact_cargo_test.py openspec/changes/report-detached-runner-cleanup/tasks.md openspec/changes/report-detached-runner-cleanup/verification.md` 实际 exit 0。源码最终 diff 为 runner +13/-8、tests +118/-13；另新增本记录并只勾选本 change 的两项 coder 完成任务，父 review/同步/归档任务保持未勾选。

```sh
openspec validate --all --strict --no-interactive
```

实际 exit 0，23 passed / 0 failed（23 items），包括 `change/report-detached-runner-cleanup`。日志：`/tmp/grow-pb04-openspec-01a1b1db.log`（任务 `01a1b1e3-01c6-7942-b5bf-c621fa3752ff`，已终态）。这是当前未归档工作区验证；父完成开发说明/归档后仍需执行对应门禁。

## 限制与交接

- 本次消除了无依据的完整 tree 确认，没有承诺能够终止任意脱组后代。正常未中断 phase 保持原行为，不增加 tree 枚举或追踪。
- 真实进程验证仅在本机 Darwin；未执行 Linux 或 Windows 原生现场。Windows taskkill 测试仅是 mock 接线证据。
- 明确未运行 `RealLibtestSelectionTests`（其 setUpClass/test 会调用 Cargo），未启动新 Rust target、未修改 Rust/dependency，也未排队 Cargo。父在唯一 Cargo owner 释放且获得相应窗口后，可单独运行 `python3 -m unittest scripts.tests.test_run_exact_cargo_test.RealLibtestSelectionTests -v`；如要求完整文件 suite，命令是 `python3 -m unittest scripts.tests.test_run_exact_cargo_test -v`。这些不是本次已通过证据。
- 父负责独立 review、`docs/development.md`/backlog 同步与归档后的验证。本次不提交、不发布，保留其他 owner 的 dirty 工作。
