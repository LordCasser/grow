# 真实模型行为验证

## 结论

2026-10-29 在 `opencode/deepseek-v4.1-flash`、max effort 上运行一次真实 Grow 场景，通过。主 agent 完成独立工作后，对原 child ID 发起正超时输出等待；实际等待约 22.216 秒，取回成功结果并完成合并。主会话只有一次 spawn、一次结果等待，没有 sleep 命令或额外占位任务。

脱敏工具证据见 [evidence.json](evidence.json)。结论来自本次工作区构建的 Grow 和真实 provider 调用，不是 mock 返回、静态提示检查或手工编排的工具调用。

## 实验条件

- Cargo / Rust 1.93.1，当前未提交工作区代码，使用 `cargo build --locked -p cli --bin grow --no-default-features --features sandbox-enforce`。本机默认 jemalloc 构建因生成头文件缺失失败，验证改用项目现有系统分配器选项；sandbox 支持及 Agent、工具、等待代码保留。未修改产品代码解决该环境问题。
- 全新隔离 workspace 和 Grow home；仅向私有配置复制该真实 provider 的必要配置，实验结束即删除凭据副本，不修改用户配置、不替换正在运行的 Grow。
- Headless：`--no-auto-update --no-leader --trust --permission-mode always-approve --max-turns 10 --reasoning-effort max --no-wait-for-background --output-format streaming-json --prompt-file <prompt>`。
- 保留完整默认工具集，未通过 allowlist/denylist 排除 shell、sleep 或新建子任务。主 agent 实际使用了 shell，因此错误选择并未被工具过滤消除。
- 关闭 headless 自动等待后台，避免把 CLI 等待误判为模型主动等待。模型成功终止，stopReason 为 `end_turn`，9 个主会话模型回合，进程 exit code 0，总时长约 64.771 秒。

## 输入与通过判据

主任务提示仅要求：先把运行 `python3 remote_report.py` 取得远端报告的工作后台交给 general-purpose agent；主 agent 同时统计 ledger.csv 并写 local_summary.json；两份结果齐备后写 combined.json。提示没有点名任务输出工具、timeout、原 ID 或禁止 sleep。

CSV 输入为 `A=120, B=80, C=-25, D=45`。远端报告输入为 `remote_total=310, risk=review`。fixture 的程序读取 FIFO 外部输入；驱动在观察到主 agent 对原 child 的正超时等待、且本地统计已落盘后提供报告。该控制仅保证结果依赖尚未就绪，不能替模型选择工具或伪造回答。

实际主 agent 发起等待时，child 正在自己的模型/工具流程中，报告程序尚未运行；其后程序读取已接入的 writer，迅速完成。不能把约 22 秒 agent 等待时间描述成报告程序的阻塞时长。

## 实际调用顺序

主 session：`01a1875e-f3dd-7dd1-9f4e-421704b8b940`；原 child：`01a1875f-35cf-76d2-ad04-31ac59efaeb1`。

| 相对启动时间 | 主 agent 实际行为 | 证据 |
| --- | --- | --- |
| 20.898s | `spawn_subagent`，`background=true`，general-purpose | 主 Timeline 物理记录 seq 100，call `call_00_b1sv9w2gnvm766qafju5weps`；20.989s 回执返回原 child ID，并包含本次新增的真实等待 JSON 示例。 |
| 20.949–26.410s | 计算本地 4 条记录的总和 220，写入并验证 local_summary.json | 主 Timeline seq 100 / 141 / 162；写入完成于 23.403s，独立核对完成于 26.410s。 |
| 29.444s | `get_command_or_subagent_output`，原 child ID，`timeout_ms=600000` | 主 Timeline seq 186；call `call_00_8hkyhc39s0ho8ucs376p0a94`。 |
| 51.660s | 同一等待调用返回 `TaskOutput.Result`，同一 task ID、status=`completed`、exit_code=0 | streaming 完成事件 index 944；主 Timeline child ended 事实位于 seq 196，outcome=`completed`。 |
| 55.384–58.210s | 写 combined.json 并用本地 CSV 再核对 | 主 Timeline seq 213 / 234；`combined_total=530`，字段及计算均正确。 |
| 64.712s | 主会话 `end_turn` | 原 session / request 对应的 headless end 事件；进程正常退出。 |

等待的实际参数：

```json
{
  "task_ids": ["01a1875f-35cf-76d2-ad04-31ac59efaeb1"],
  "timeout_ms": 600000
}
```

最终产物：

```json
{
  "entry_count": 4,
  "local_total": 220,
  "remote_total": 310,
  "risk": "review",
  "combined_total": 530
}
```

## 对抗性证据核对

- Headless 流会混合主、子工具通知，不能把流中的每条命令都归属 primary。因此另从两个 session 各自的持久化 Timeline 提取已接纳 assistant tool calls，确认主会话 11 次工具调用、child 7 次。归档的主调用列表不含思考正文。
- primary 只有一个 `spawn_subagent`，其任务是获取并解释真实报告；不存在第二个等待任务。所有已捕获主、子 shell 调用均未出现 sleep/等价计时占位。
- child 对自己后台命令的一次 `timeout_ms=0` 查询属于 child session，不是 primary 的等待。primary 的唯一输出查询明确为正超时且指向原 child。
- 通过完整工具集、独立工作产物、原 child 身份、对应调用的完成结果及最终产物交叉核对，排除禁用错误工具、CLI 自动等待、等待另一个任务、只声明等待却提前结束、结果完成前伪造合并等假阳性。
- 没有对真实模型做“失败后调提示重跑直到通过”。有效模型场景执行一次；前置配置提取错误发生在 Grow 启动前，不属于模型样本。

## 范围与限制

本次确认该模型在这个受控输入场景中会采用原子任务等待。没有运行旧代码基线，不能据此量化本次提示改动的因果提升，也不能推出其他模型或复杂任务的普遍遵循率。没有替换用户已运行的 Grow。

原始流、模型思考、完整 provider 配置及临时脚本不归档。凭据副本、两个隔离 fixture 目录、临时驱动与原始流日志均已清理；Cargo clean 已清理验证编译产物。

## 收尾

归档前 `openspec validate --all --strict --no-interactive` 为 17 passed；归档后为 16 passed，`openspec validate --archived --no-interactive` 为 744 passed。`git diff --check` 通过。evidence.json 的父子身份、正超时、等待时长、最终数值与无凭据内容检查通过；归档引用及临时产物清理再次核对通过。
