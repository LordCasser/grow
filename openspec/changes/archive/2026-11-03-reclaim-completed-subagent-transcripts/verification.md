# 验证记录

最终验收使用项目固定 Rust 1.93.1，并显式指定 RUSTC/RUSTDOC，避免系统 PATH 中的 Homebrew 1.99.1 被 Cargo 间接调用。测试复用共享 target，限制 `-j 2`，没有另建完整构建目录或改写个人工具链设置。

## 验证范围

- `subagent_reclamation`：顺序完成多批带正文的 child、对象容器清空、重复重开、真实 Esc 离开、选中/运行中保留、draft/control/highwater 保留、媒体缓存释放、父/子 replay 和 viewer owner、permission responder 存活。
- 损坏 journal 与移走整个 session 目录：错误有界、loaded=false、实时尾部不阻挡后续回放、源恢复后任务/工具/终态各一次且旧错误提示消失。
- 子 agent 既有生命周期、cold replay、控制投影、后代恢复与 tracker 行为可用性；避免只验证新增 helper。
- 后台任务：持久与晚到 completion 的 output 均为空，stdout 只来自原 live MonitorEvent；回收后历史恢复开始/完成行和链接，canonical completion 收敛 Running owner，重复通知不重建/回退已持有的任务状态。
- 本轮未在共享工作树中临时替换为旧实现执行红测，以免其他并行构建读到故意退化的实现。回归先确认正文已驻留，再断言回收后为空；旧路径没有相应清理，不能满足此断言。不把该静态核对声称为已运行的旧版红测。

## 环境与中间失败

首轮编译受并行 inquiry schema 改动的两个测试夹具缺 `answer_context` 阻挡；并行任务随后已补齐，撤回本轮临时补值，避免重复字段。另修正新增 viewer 回归的模块引用。Homebrew 编译结果和中止的待锁构建不计入最终通过记录。

## 最终通过记录

使用显式固定的 Rust 1.93.1 执行 `cargo test --locked -p pager --lib subagent -j 2`：**200 passed，0 failed**，包含全部 9 项新回归。构建完成后对同一 `target/debug/deps/pager-e8d874d24401761f` 测试产物运行以下相关分组，避免并行任务改动触发重复编译；各组有交集，不相加作为独立总数：

| 过滤器 | 通过 | 失败 | 忽略 |
| --- | ---: | ---: | ---: |
| `background_tasks` | 22 | 0 | 0 |
| `descendant_recovery` | 7 | 0 | 1 |
| `tracker_retains_shell_behavior_availability_projection` | 1 | 0 | 0 |
| `minimal` | 80 | 0 | 0 |

后代恢复忽略项是原有 benchmark，不作为已执行通过。没有执行全 workspace 测试、实际 TTY/50G 故障重现或宣称固定峰值/RSS 降幅。对象回收依据是已加载正文容器清空和从同一持久历史的完整往返。

最终指定文件的 Rust 1.93.1 rustfmt、`git diff --check` 通过。归档前 `openspec validate --all --strict --no-interactive`：19/19 通过；并确认两个 MODIFIED 条款均保留现有全部场景。

已执行 `openspec archive reclaim-completed-subagent-transcripts --yes`：client-surfaces 新增一个要求、修改一个要求，原有 deferred history 场景全部保留。归档后全量严格校验 17/17、归档校验 804/804 均通过；清单与 developer 文档引用均核对。已从 backlog 移除本轮闭合项。

清理了本轮 14:24 失败编译留下的一个 237 MiB 未完成 incremental session，删除前确认对应 lock 无活动 owner；没有执行全局 cargo clean 或清除并行任务的构建目录。
