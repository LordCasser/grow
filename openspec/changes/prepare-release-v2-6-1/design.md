# Release preparation boundary

当前工作区承载本轮已归档但未提交的功能、修复和用户预先改动。准备版本前先以已有change和实际验证分清交付事实，不能把dirty视为已发布或丢弃用户工作。[里程碑记录](../archive/2026-11-05-coordinate-audit-backlog-repairs/milestone.md)记录本次停止点；尚未实施的输入/插件/MCP/ClientFS/PTY/crash等保留backlog，不在2.6.1说明中声称修复。

在用户指令到达时，索引WS01已通过公共Handle red/green与57项lib，原在途Luna窄review随后无可行动问题；Shell取消终态修复已开始必要local→新defaultCLI→原exact链，继续到既定闭环，禁止扩出新审计/修复。用户停止新审查后不再派独立review，Shell最终由父检查真实生产diff和运行证据，该范围调整写入独立change，不能假称新独立review。

版本候选按2.6.1准备。版本维护必须等现Shell Cargo及源码窗口退出并明确释放后再改manifest/lock/编入产物的changelog，以免旧编译和测试混用源版本。沿用workspace.package.version继承，独立版本crate和third-party不改；用既有Cargo离线更新本地lock元数据，不升级外部依赖。小型版本维护不需要新业务抽象。父写架构/验证/发行文档；实际版本维护若委派，仅该维护项和必要调用方元数据同步，不再派审查任务。

用户随后补充要求发布前清理编译warning和cargo fmt：固定1.93.1执行`cargo fmt --all`及最终check，保留用户dirty的行为，单列纯格式文件。生产与workspace/all-targets编译日志不得留未处理warning，不用全局allow/suppress掩盖；实际诊断导致的必要修正按其owner最小处理，不扩大为新审计或全仓clippy。原受测hash若因纯格式变化，保留旧证据和前后差异并绑定最终候选hash。

验证先复用各change已接受的实际证据，不重跑全部局部tests。版本更新后运行锁定metadata/版本边界、发布脚本既有测试和最终默认CLI生产check/build，显式产物版本及source snapshot绑定到本地候选；仓库既有core/跨平台/PTY/attestation门禁按精确committed SHA进入正式发布流程，本地未执行就明确待CI，不能以新一轮全仓审查替代发布门禁。源码中已知未闭合问题、旧live host finalization和进程树清理限制如实列明。

实际workspace/all-targets检查exit0，但`test_subagent_effort.rs::script_parent_todo`丢弃must_use `InferenceExpectation`产生两条warning（另有Cargo汇总）。该helper属于测试fixture，现三个调用点都应返回并保留expectation，在对应parent prompt完成后调用`assert_satisfied`；不改响应匹配、工具参数、持久契约或生产代码。此项由单独coder只改该fixture，随后重跑本target编译及最终workspace/all-targets警告门禁。清理warning是用户明确要求的发布维护，不是新增架构审查。

## Core CI fixture alignment

官方run42020391094的Core job127221555801在Shell库唯一失败：`stop_wins_when_the_last_planning_dependency_resolves`仍要求已durable Stop后actor保留旧cursor（Some），实际None。父已读完整测试、取消owner和已归档model-sampling终态退役条款；None符合新契约，原用例的Stop胜过迟到依赖、单Turn/不重采样/晚交互持久事实仍须全部保留。此修复仅由Shell测试owner更新该测试：停止前确证原cursor存在且Turn身份一致，ACK成功后turn/sampling continuation均None；Question/Permission迟到结算后仍None，且所有原无重采样及持久观察断言保留。生产代码、权限/Timeline校验、provider行为及其他用例不改，也不以ignore或删除场景解门禁。

验收先用原场景确认有意义red，然后同exact green；运行取消owner组和挂起cursor既有exact，以区分终态退役与同Turn挂起保留；最后完整Shell库4线程通过并核warning/fmt。父读实际diff/日志，不新增独立review。完整Core后续包、Settings、原生恢复和PTY在最终精确提交的官方门禁重新执行，当前失败运行不算通过。现远端v2.6.1已绑定首候选；测试修正候选与验证完成前不移动该共享tag，发布身份变更单独处理。

## Linux OCR fixture startup

第二轮official run42033135390的Core job127264627813在同一e6dafd23上失败，而push Core曾完整成功。唯一失败为`local_process_retains_text_and_rejects_empty_or_failed_output`：line1375临时脚本启动收到ETXTBSY（26），不是ENOENT（2）或缺少tesseract。测试`executable`自行写入`ocr-test`并chmod；并行fork可能暂持该写描述符，已有hooks测试也明确记录此机制。生产`run_local_ocr`直接spawn程序并分类错误，必须保留；不安装真实OCR、不在workflow跳过、不放宽原断言。

本次只在`#[cfg(all(test, unix))] local_ocr_tests`内增加必要的临时脚本启动适配：固定有界次数/总时间，只重试`local OCR unavailable`的ETXTBSY，失败发生于exec之前、尚无OCR子进程；任何ENOENT/EACCES、退出失败、空/超限输出或进程组错误立即返回。局部script fixture各启动入口复用该规则，外层现5s超时、取消/进程树guard和PID握手不改。禁止无界重试、通用spawn重试库或生产降级。最终持续busy仍失败，不把environmenterror变成功。

必要验证是Linux受控writer持有造成真实ETXTBSY、release后成功、持续busy不启动脚本和返回错误，以及非busy错误不被重试；在支持平台跑本OCR组（包括取消/超时/后代清理/原四输出场景）、格式warning门禁。macOS通过不能证明Linux控制，Linux证据应来自既有CI或可达隔离环境，未实际运行则明确待验。父检查生产段字节相等及实际日志；最终精确提交Core仍需全门禁，不以偶发成功替代机制验证。已失败的第二轮按用户停止指示取消，并确认completed/cancelled、零unfinished。

## Minimal Ctrl+D PTY failure boundary

第三轮official42048771667在c81c8d87的Core/PTy唯一失败为`minimal_ctrl_d_confirms_and_quits`：确认提示已出现并用Ctrl+C清除，随后单write送`\x04\x04`，5s后process仍Running；失败screen仍idle，不能据此区分按键/确认未交付、退出挂起或等待方法未pump。主规范`Confirmed quit requests bounded session tree suspension`要求一秒内同键二次确认、原五秒绝对suspension期限、不自动force，不能为CI改窗口或接受Running。`wait_exit_code`仅poll/sleep，不处理PTY输出或终端query；已有`wait_for_exit_and_drain`会pump并保留实际exit/status。现证据不证明生产错误，也不证明仅延长等待可解。

先由Pager PTY fixture owner复现并收集实际输入/屏幕/进程事实。优先仅同test文件同步现有harness输入和输出、用已有可pump且实际exit0的等待；必须保留Ctrl+Cidle不退出、首D可见确认、另键清arm、第二D确认、无force/kill、原一秒及五秒生产边界。夹具的观察期限应覆盖退出/终端收尾，不能仅与产品suspension5s相同，也不能只改数字无证据。若证据指向production/公共harness缺陷，先报告其所有者与路径，父修订任务后再实施；禁止静默扩业务或新抽象。定向原exact与必要控制足够本地验收，Linux最终CI仍需真实执行；不跳PTY门禁、不整轮盲重跑。

正式命令依据`scripts/validate-release.sh`和`.github/workflows/release.yml`：验证annotated tag对应HEAD且版本匹配；从main dispatch全部Linux/macOS/Windows/OHOS组，发布十个平台资产及SHA256SUMS并核attestation。用户随后明确授权“按2.6.1准备候选和发布”，因此本change包含必要的候选commit、annotated tag、push和该版本的官方workflow/Release发布；先完成本地具体候选及验证，再逐步执行，不修改门禁、不force push或重写已发布tag。

## Sampler actor terminal fixture

Responses 修复之外的实际 sampler integration 执行在 test_actor 出现六项终态超时。源码显示 request_task 成功只发送 RequestCompleted 元数据，完整 ConversationResponse 经 completion oneshot 返回；原测试 drain、并发完成观察和正文断言仍等 L2 Completed。先以 exact 与原 decoder 对照确认此差异，不能只因超时宣称负载问题。若证明为基线旧夹具，独立 test_actor owner 修正这一个共同根因，不混入 client decoder 任务。

测试应通过 actor 的 RequestCompleted/Failed 观察终态，并按对应 request identity 验证 metadata；需要正文/usage/stop reason 的原断言通过现 submit_and_collect 返回值核对，正文仍须完整验证，不能仅用 item_count 取代。保留 AttemptStarted/StreamStarted/FirstToken/ChannelToken 顺序和内容、重试次数、并发 request 身份、Messages refusal、custom-frame/idle/error 的原有边界。不得让生产重新复制完整响应到事件通道，不同时接受两种成功终态作兼容，不延长 timeout、不删失败/正文断言。先回归失败 exact，再完整25项该二进制与相关sampler integration，固定1.93.1 fmt/warning检查。此项仅修测试对既有执行接口的旧假设，不改运行契约，纳入本发布维护 change。
