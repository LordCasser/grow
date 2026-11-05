# Verification

状态：tools与Shell模块、独立review和当前CLI跨特性里程碑均已验收。旧set_session_model自身绑定已被目标child工具替换，本页记录本change独立证据；归档校验另列。

## Tools module

公共接口按design落在新set_subagent_model模块/原session-bound ChannelBackend/coordinator/ChildRunner。父已亲自核对schema、注入source和active direct-child转发条件，以及Shell接收exact handle的生产候选。工具只等目标actor admission ACK，不等application；历史自身工具源码/注册无别名，默认Agent toolset三处与workspace穷尽InternalControl arm同步。

Rust1.93.1，PATH优先$HOME/.cargo/bin，RUST_MIN_STACK=16777216，三次均独立临时GROW_HOME。coder实际命令 `rustup run 1.93.1 cargo test --locked -p tools --lib <filter> -- --test-threads=1`：

| Filter | Actual result |
| --- | --- |
| set_subagent_model | 4 passed，0 failed，2458 filtered out |
| model_control | 9 passed，0 failed，2453 filtered out |
| registry::tool_kind_contract | 2 passed，0 failed，2460 filtered out |

隔离home为/tmp/grow-tools-subagent-model.99NsVP、/tmp/grow-tools-model-control.r44jQw、/tmp/grow-tools-model-registry.dphtLS。未编Shell，不把模块通过当跨crate完成。

测试覆盖strict fields/空target/至少一项/null/none、ownerACK/拒绝/closedACK、host-bound source/unbound observer拒绝；coordinator exact runtime/参数/token转发且邮箱不因等ACK阻塞；self/sibling/root可见grandchild/foreign/unknown/initializing/cancelled/settling/suspended/completed/failed/Workflow/finalization_pending拒绝且不恢复重跑。父核对新增coordinator tests含真实session-bound native→coordinator路径、history visibility不授control与原角色caller上下文。

## Shell verified candidate

最终正确工具链命令：

```sh
PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:$PATH" \
RUSTC="$HOME/.cargo/bin/rustc" RUSTDOC="$HOME/.cargo/bin/rustdoc" \
GROW_HOME="<private tmp home>" RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 \
cargo test --locked -p shell --lib session_model -- --test-threads=1 --nocapture
```

Cargo/rustc/rustdoc实际-vV均1.93.1。父读取完整成功日志并独立工具确认task01a1a969-e77a-76c3-bf1b-9cd9f48a4c59 completed/exit0：11 selected，11 passed，0 failed，4577 filtered out；tests114.08秒，总167.63秒。native外层包含21个隔离真实parent→child provider/race case，owner外层含10个隔离case；不将它们误记为31个Cargo顶层tests。筛选同时覆盖原ACP/user路由、authored eligibility与exact one-shot permit。

实际native用例覆盖model/effort组合/null/none/pending/rebuild、unknown/private/hidden/unsupported/forged/empty/malformed、eligibility/permission/Hook拒绝；父route/出生事实不变；目录锁等待自然完成（有首次check到达oneshot握手）与两selection的mailbox-after-finish零接纳/模型事实回归。actor最终native-only can_continue_regular_turn在同gate/state guard中先于slot/ACK，无await；ACP None保留idle选择，luna静态复审确认原TOCTOU闭合。

中间正确工具链轮11 selected、10pass/1fail，native both case读取已退休child ChatState失败；测试改用正式storage读取closed child durable Timeline，另将read_file参数path改为实际target_file，无production变动或放宽路由/queued/provider断言。最终原filter全部通过。既有session-end Hook的closed Timeline警告单独保留，不作为当前失败根因，不顺带修。

## Integrated CLI acceptance

父以当前合并源码运行 `/tmp/grow-final-model-compaction-checks.py`，显式固定Cargo/rustc/rustdoc1.93.1、RUST_MIN_STACK=16777216、CARGO_BUILD_JOBS=2，各阶段新建独立GROW_HOME。task01a1a977-4e43-7631-bdb1-876d1d9e1f05真实completed/exit0，总347.35秒，日志根 `/tmp/grow-final-model-compaction-7rsawkm8/`：

| Command | Actual result |
| --- | --- |
| cargo check --locked -p cli | exit0，59.51秒 |
| cargo build --locked -p cli --bin grow | exit0，108.08秒 |
| cargo test --locked -p shell --test test_subagent_effort -- --ignored --test-threads=1 --nocapture | 3 selected/3 passed/0 failed/0 ignored，tests30.93秒 |
| cargo test --locked -p shell --test test_released_delivery_finalization -- --ignored --test-threads=1 --nocapture | 1 selected/1 passed/0 failed/0 ignored，tests6.82秒 |

新CLI SHA256 `c5874039745b1ce07d42ef111973abc60b4754f1ba22ed816f812dbeb8abe2d0`。三项真实进程覆盖effort支持/拒绝、resume独立字段覆盖、parent控制运行child→下一目标endpoint/effort→child完成→省略字段resume继承unset effort，核对父route与source birth/Timeline immutable。Goal用例由真实native工具创建子任务、Released(None)、父修改、活跃期Complete拒绝、kill/wait自己的测试child，再冷核对Spawn/Seed/Result/Ended、Unapplied replan和精确Goal报告。父亲自读实际日志；不将该隔离Goal通过当作正常旧宿主恢复。

独立review发现的actor竞态与目录锁测试握手均已闭合，父接受tools/Shell候选及最终21provider+10owner运行证据，默认CLI组合里程碑通过。文档和相关delta随本change归档，最终strict/archive结果记录在下方。

## Earlier failed build record
首次Shell命令798.84秒后编译exit101、实际selected=0，没有运行通过证据。两项测试编译错误为同名list_running方法应显式SubagentBackend trait调用（原owner已修）和compaction独占测试缺stack helper（交其owner）；生产helper/actor guard在该轮没有编译诊断。另纠正工具链：用了rustup Cargo1.93.1但PATH仍使子rustc为Homebrew1.99.1，该轮不能标为Rust1.93.1验收。下一轮同时显式PATH/RUSTC/RUSTDOC固定rustup并核对-vV。其他已明确PATH隔离的tools/ChatState定向证据不受这次错误归因影响。

## Isolation and limits

所有运行在测试自建GROW_HOME/loopback provider。正常Grow会话、全局安装binary和正常session存储未更改；没有Git提交、发布或安装。当前运行宿主不会因本地源码验证自动获得新工具，不把隔离结果等同正常旧会话终态已恢复。

## Final format and specification validation

父最终全workspace Cargo fmt发现两处本feature换行差异，原owner以Rust1.93.1只修model_switch hidden return与coordinator测试map格式，无语义变化；父再跑 `cargo fmt --all -- --check`、`git diff --check` 均exit0，原行为证据有效。归档前 `openspec validate --all --strict --no-interactive` 19/19；本change与同批compaction/Windows修复归档合入各主规范后，全量strict16/16、`openspec validate --archived --no-interactive` 853/853，0 failed。全部review绑定按宿主实际canonical result接受；不伪造旧finalization_pending的结果或replan。
