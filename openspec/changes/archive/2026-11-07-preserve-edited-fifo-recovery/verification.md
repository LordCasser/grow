# Session F2 · FIFO 编辑恢复验收

状态：父已分别接受 T（ChatState 顺序）与 S（Shell 行运行身份），并核对组合的真实 JSONL 编辑/恢复链。已归档为`2026-11-07-preserve-edited-fifo-recovery`，规范门禁见末节。不表示整个 backlog 或 2.6.2 已完成。

## 两个 owner 的结果

- Timeline private `InputFold.canonical_position` 在 Submitted 初始化、成功 AdmissionResolved 后继承 supersedes 最早槽位；查询沿原 pending 过滤按该序输出。父对照实施前保存基线核本轮diff，原 validator/prepare/accept/replay原子性保持，无新增schema/事件/公共registry。有效旧红中 A/B/C 编辑 A 后重建与公共Actor查询均为 B/C/A′；新绿4项事件恢复＋1项Actor查询，150项fold和26项input回归通过。详见[canonical证据](verification-canonical-order.md)。
- Shell FIFO恢复在既有queue_meta构造后，以其id恢复InputItem.prompt_id；无queue fallback仍为payload.prompt_id。F1 Steer分支、immutable input/payload/hash、最新正文/图片/版本来源保持。有效旧红已到实际运行身份断言：顺序/行id/inputid正确、runtime却是edit请求id。修后真实JSONL fixture通过，input-admission19、Steer2、queueedit16回归通过。详见[恢复证据](verification-restored-identity.md)。

两组filter可能重叠，不相加称独立总数。S首次新fixture编译类型错不是产品旧红；最终有效oldred保留。既有编辑编号实际为A `edit:3`、B `edit:2`，fixture按producer适配，未顺带修改编号政策。S check首次窄PATH下openspec不可见exit127，修正PATH后实际22/0，记录保留；父另做归档前门禁。

## 场景与证据强度

| 场景 | 已接受证据 |
| --- | --- |
| A早于B/C而编辑较晚，仍恢复首位 | T真实Timeline/Actor回归＋S真实queue_input/HoldSave→JSONL durable ACK→owner销毁/新writer→restore |
| A两次、B一次；每次成功接纳切点 | T每次success事件序列JSON往返/Actor ACK后查询；S最后切点真实文件恢复。每条被替换身份仍RouteSuperseded，canonical pending只含最新3项 |
| 接纳未成功/拒绝 | T未决/prepare-only/Hook Block不替换旧Allowed行；原validator对非法supersedes拒绝原子性；S运行既有拒绝save/hold与F3 unknown/physical-writer回归 |
| 内容/身份保真、Hook/消费一次 | S检查原runtime/rowid、latestpayload/hash/inputid、图片、client/screen/verbatim/json_schema、版本；恢复事件数不变，Hook每input一次，随后真实Turn预留/consume仅消费A′ |
| 原插话与稳定重试不退化 | F1 preserved/closedSteer两项、F3/input-admission19项、queueedit16项实际通过 |

## 审查与限制

父亲读两个最终生产改动及fixture、有效oldred/green/回归日志。顺序归Timeline，Shell不另扫raw排序；queue.id与payload.prompt_id归属明确；没有混入reorder持久化或模型控制工作。无需更改公共返回类型、配置、磁盘格式或兼容路径。开发说明已同步。

S冷恢复是同进程中的真实JSONL、新storage writer和新ChatState owner，不是独立OS进程，也未完整重跑生产bootstrap的recover_interrupted_durably阶段。T每个cut是事件JSON/Actor ACK证据，不冒充每个cut均完整磁盘fsync/进程重启。真实PTY、其他平台与额外unknownACK窗口未新增；原故障与F1回归提供相邻控制。手动reorder的重启持久化是独立政策，本项不承诺。

## 规范门禁

父归档前全量strict为22 passed/0 failed（`/tmp/grow-fifo-parent-strict.log`）。`openspec archive preserve-edited-fifo-recovery --yes` actual exit0，向input-admission新增一项requirement（`/tmp/grow-fifo-parent-archive.log`）；归档后全量strict21 passed/0 failed（`/tmp/grow-fifo-postarchive-strict.log`），archived strict906 passed/0 failed（`/tmp/grow-fifo-postarchive-archived.log`），均actual exit0。开发说明和backlog已同步。无commit或发布。
