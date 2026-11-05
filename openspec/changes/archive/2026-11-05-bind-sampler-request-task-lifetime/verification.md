# Feature acceptance

SAMPLER-1模块实现与定向验证已验收，后续CLI取消/重试整体里程碑和归档仍待完成。原执行被host取消时源码保留；续跑只核对并测试，没有改生产源码或修fixture。父已亲读四文件生产diff及完整14项新公共Handle回归，确认以JoinSet tracked task ID清理当前登记、每提交token和两个collect DropGuard只取消本次工作；公开RequestId取消/查询语义及旧任务usage/evidence结算owner保持。

固定Cargo/Rust/rustdoc1.93.1、RUST_MIN_STACK16777216、JOBS2、offline、macOS/aarch64；`/tmp/grow-sampler-lifetime.qUSihE`隔离GROW_HOME/TMPDIR/空hooks，Git global/system为/dev/null。全部命令先实际构建，再执行非零测试，均首次exit0：

| 定向命令（cargo test --locked --offline -p sampler） | 实际通过 | 日志 |
| --- | ---: | --- |
| --test request_task_lifetime -- --test-threads=1 | 14 | 01-request-task-lifetime.log |
| --lib actor:: -- --test-threads=1 | 44 | 02-actor-lib.log |
| --test test_actor shutdown -- --test-threads=1 | 3 | 03-existing-shutdown.log |
| --test test_actor cancel -- --test-threads=1 | 3 | 04-existing-cancel.log |
| --test test_actor submit_and_collect -- --test-threads=1 | 1 | 05-existing-collect.log |

父已逐份读回全部测试枚举：合计65次执行、64个不同测试，0 failed/0 ignored；owned_shutdown_cancels_and_joins_in_flight_request被shutdown与cancel两filter重复命中。新测试实际14项，修正早期15项计数误报；handle.rs没有现有unit模块，未把空筛选作为通过。原actor44覆盖已有provider admission、scope/usage/evidence、取消、deadline/retry、panic owner及state行为；这些与新入口回归合并，不重复所有CLI/平台测试。

新公共Handle测试用真实SamplerActor/MockInferenceServer和有限oneshot/HTTP握手：旧usage ACK晚结束不删新登记，旧plain/accounted collect返回或Drop不取消替代项，自身Drop取消，actor处理前Drop不发HTTP，旧task与独立panic清exact登记且followup继续、无伪成功usage，normal/cancel/shutdown各结算一次。父读完shutdown对照：current ACK释放后仍等待replaced ACK，未detach旧任务；无sleep猜顺序或直接改内部map冒充公共链。

父执行shasum --check五文件全OK，与原候选和最终受测源码一致：actor/mod.rs `1e52fc8b2caa355be22181c9f01dc0382545a70b63523686c9ea7d3e8285e659`；actor/state.rs `4bc4d929ed4441c552a85aa452cda2b793c8cea8ae20a23d2528252e3b68a5a5`；commands.rs `f93315e5c71973c3e70df1ac9db7669b47185679cae6b5541aadd53688de1500`；handle.rs `76b4b43c5377ed715e4ee5917d4f583ea0f0993594c91bcfc071f881795f16b4`；request_task_lifetime.rs `c3628964486422d687470729057d318907c0f9c5889116ffac4d9ad9d87c5ed1`。限定rustfmt/diff检查通过，Cargo和sampler写scope已释放；完整命令、屏障、版本与枚举存同/tmp目录manifest.md/source-sha256.txt。

独立Luna `01a1aa58-28e6-72e1-83b4-29825d0a756b`已完成task identity/JoinError/四个Submit入口/DropGuard/shutdown及相邻request_task审查，无运行时阻塞。它查官方Tokio1.53.1与tokio-util契约确认tracked taskID和drop_guard语义，未运行Cargo，引用的64唯一测试为父实际读回证据。

唯一低优先级发现是request_task.rs原doccomment仍声称返回RequestId经join_next清active map。父亲读确认并独立派`01a1aa63-aa27-7093-9451-86fea7b11e7e`限域修注释；最终diff只将其改为taskID经join_next_with_id/JoinError::id清理，逻辑RequestId仍用于请求/事件。父核diff、空白检查与SHA256 `dcc56f5a7de8a90e126ebad897a7341213b290146629bb2608af7d8bd265a5bb`，没有业务变更，未重复已过测试。原五受测文件指纹保持，此第六文件是纯注释适配。

父接受模块功能和该独立修复，勾选实现/公共Handle验证与独立review任务；后续CLI取消/重试整体里程碑、开发说明/规范归档与backlog移除仍未完成，不宣称整体backlog通过。未触正常Grow存储、真实provider/MCP/hooks、用户工作树、安装/重启或Git提交。

## 当前CLI里程碑候选

独立coder `01a1ab31-786c-7fe3-9f1d-e37b0e243a02`只写新Shell integration test `test_sampler_request_lifecycle.rs`，生产Sampler及共享harness不改。父亲读整个候选：原生ACP正常prime→终态前barrier阻塞的旧工具候选→session/cancel→同一resident session新prompt 503后成功→正常follow-up。核旧工具不dispatch/history不含candidate、同PID/controlEpoch、四个逻辑RequestId和五个实际attempt、取消与503保持未知用量、其余三个可信usage及exact evidence/settlement身份。公开重复RequestId反例仍由模块64unique回归证明，不把Shell随机RequestId产品路径当作原bug复现。

运行使用TODO最终新defaultCLI `0cb021c4e645fc662464f5b679ca738e3e3733c3632464b89f9c27c3d3bce7d8`，显式GROW_BINARY、固定1.93.1/offline/locked、独立`/tmp/grow-sampler-cli-verify.0znoP9/`环境。前两轮均编译后0pass/1fail、actualexit101：第一轮raw扩展wire漏`_`前缀，prime前prompt_status MethodNotFound；第二轮prime已完成但fixture用可选`x-grow-turn-idx`筛请求误得0。修正仅按现wire_ext_request和既有foreground tools数组规则核请求，保留全部5次请求与生命周期断言；失败log/results及2337项source前后稳定记录保留。

第三、四轮实际exit101，均在取消/新请求成功/后续正常请求、5次HTTP、同PID/controlEpoch、干净history通过后发现新Turn attempt继承旧计数，Completed为[1,3,1]。第四轮诊断明确新RequestId的503为attempt2、成功为3，不存在该ID:1，五次实际attempt均有各自settlement/evidence。父亲读真实取消路径，对照正常Turn epilogue并独立解析全部日志、四轮source快照和最终源/二进制hash，记录`parent-review-summary.json`，确认另属Shell取消terminal owner漏清sampling continuation，已拆[独立修复](../2026-11-05-retire-cancelled-turn-sampling-state/design.md)。本模块验收保持，CLI里程碑仍未通过，不勾2.2或移除Sampler backlog。原测试保持[1,2,1]失败断言，title Sideband证据需按foreground RequestIds过滤，交该独立修复同步。原verifier已终态且释放Cargo/sourcescope，父仅解除这份部分候选交付，不将其接受为产品通过。

第四轮关闭session ACK成功，直接CLI进程PID83442由harness SIGTERM回收（143）且独立kill(pid,0)确认absent；harness保留descendant-cleanup EPERM和tree_attached=false，不能据此声称全平台后代清理已证。日志SHA256 `054c46111eabe6f3b0ec0e418963f09a9194284da04cadd9e092cd926f3df1bf`，最终测试SHA256 `58838cee4e9676c37ed14b1326a56aeeb7c3472b2ce46614946426405be7ba7b`。

## CLI milestone accepted after the separate Shell repair

The separate [cancelled-Turn retirement](../2026-11-05-retire-cancelled-turn-sampling-state/verification.md) repair completed on final Shell source and a newly built explicit default CLI (`aecdaab85a17e957c0348f4644ed1c86a86f5ae120cbb400f589f53109d827e6`). The original native cancel → same-session 503/success → normal follow-up exact actually passed: one test, no ignored/filtered results, exit0. Completed attempts were `[1,2,1]`; all five foreground HTTP, settlement and response-evidence identities agreed, three known and two unknown usages retained, old tool undispatched and history clean, resident epoch/PID preserved. Foreground evidence is filtered by its actual RequestIds, retaining the title Sideband separately.

Parent read the final diff and actual phase logs, recomputed final hashes and independently parsed the five identity keys in `/tmp/grow-retire-cancelled-turn.GL3wwe/parent-review-summary.json`. All 2338 Rust/manifest/lock hashes were stable during each run. Final cancellation checks 4/4, existing recovery-budget 8/8 and suspension-cursor exact1/1 support the distinct Shell change; they are not counted as additional Sampler module tests. Sampler's original 64 unique module tests and independent review remain the feature evidence. Native CLI cancellation/retry is now accepted as the combined product milestone.

This closes the outstanding milestone for this task-lifetime change. Developer explanation is synchronized, the two resolved backlog entries are removed, and the accepted source proceeds to local strict/archive checks. User stopped new reviews during the Shell fix; parent accepted it without dispatching a new Luna review. Existing direct-process/harness cleanup and unexecuted platform boundaries remain explicit.
