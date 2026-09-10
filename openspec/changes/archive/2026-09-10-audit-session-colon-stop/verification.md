# 验证记录

## 已确认

- 原 session 的 Timeline 共读取 49670 条事件；对 seq 49584、49601、49617、49637、49660 的响应拼接按 observation bytes 校验，直接解析原始 SSE。三次停顿为合法 end_turn/message_stop；另外两次为完整 tool_use。
- 对实际请求 seq 49583、49600、49616、49659 检查工具定义、输入 block 和尾部消息。后两个请求各有一个孤立 tool_result，均不存在同 ID 的 tool_use；首个停顿无孤立结果。
- 核对最近压缩、模型切换、Goal 暂停/清除及 TurnEnded；没有把 cancelled 与 completed 混为同一故障。
- 比较 `155780e2..162ba0ba`：未修改 native 缺失时的 prefix 切点及 request_segments 的两侧投影逻辑。压缩续接修复的准入条件不适用于事故 Step。

## 当前代码的最小复现

先用锁定依赖从当前源码构建库，随后链接该产物执行独立诊断程序：

```sh
cargo build --locked -p sampling-types --message-format=json
rustc --edition=2024 -C panic=abort \
  openspec/changes/archive/2026-09-10-audit-session-colon-stop/projection_probe.rs \
  --extern sampling_types=target/debug/libsampling_types.rlib \
  -L dependency=target/debug/deps -o /tmp/grow-colon-projection-probe
/tmp/grow-colon-projection-probe
```

执行时探针位于归档前的 `openspec/changes/audit-session-colon-stop/`，上面路径已更新为最终归档位置。库构建成功，耗时 29.93s；探针三个断言通过：

```text
prefix=2: tool_use=0, tool_result=1, historical_exchange=0
prefix=1: tool_use=1, tool_result=1, historical_exchange=0
prefix=3: tool_use=0, tool_result=0, historical_exchange=1
```

第一行复现缺口，后两行为同一工具往返完整位于 prefix 之后或之前的对照。探针只调用生产 `build_messages_request`；没有伪造其转换实现，没有执行原任务工具，没有请求线上模型。ChatState 设置 prefix 的调用时序由源码和原始 wire 交叉核对，本次没有新增完整 SessionActor 回归。

## 失败与限制

- `grow trace <id>` 触发既有总字节上限，未导出完整 trace；改为只读原始文件中的相关事件和 chunks，没有放宽导出限制。
- 独立 rustc 第一次使用错误的依赖目录，报找不到 crate；第二次缺少 workspace 的 panic=abort 设置而失败。修正为 `target/debug/deps` 和 `-C panic=abort` 后成功，未改产品构建配置。
- Atlas 对部分源码存在的符号返回空结果，具体控制流以直接代码读取为准。
- 本次没有重跑全 workspace 测试、没有线上 A/B，不能证明修复投影后模型一定继续，也不能把首个无孤立结果的停顿归因于这个缺口。
- 工作区另有他人新增的 Summary 投影读取边界 backlog 条目，本次保留。

## 规范校验

- `git diff --check`：通过。
- 归档前 `openspec validate --all --strict --no-interactive`：19 passed，0 failed。
- `openspec archive audit-session-colon-stop --skip-specs --yes`：成功；归档操作自身的最后一项任务随后勾选。
- 归档后 `openspec validate --all --strict --no-interactive`：18 passed，0 failed。
- `openspec validate --archived --no-interactive`：310 passed，0 failed，包含本审计。
- 未改运行逻辑、配置或原 session，未提交 Git、未安装新版本。
