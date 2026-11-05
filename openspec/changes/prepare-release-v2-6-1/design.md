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

正式命令依据`scripts/validate-release.sh`和`.github/workflows/release.yml`：验证annotated tag对应HEAD且版本匹配；从main dispatch全部Linux/macOS/Windows/OHOS组，发布十个平台资产及SHA256SUMS并核attestation。用户随后明确授权“按2.6.1准备候选和发布”，因此本change包含必要的候选commit、annotated tag、push和该版本的官方workflow/Release发布；先完成本地具体候选及验证，再逐步执行，不修改门禁、不force push或重写已发布tag。
