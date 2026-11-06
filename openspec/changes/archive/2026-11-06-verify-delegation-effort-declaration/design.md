# Evidence boundary

TaskToolInput的JsonSchema经registry生成和finalize、tool别名渲染与Agent动态description，进入每次provider request。原会话seq17664/17679请求的spawn_subagent schema相同，properties无effort，required仅prompt/description；统一日志对应pid46777/ver2.6.0。当前安装命令的commit不能无条件归到原PID，但日志足以证明旧运行版本。

2.6.1 annotated tag剥离到deaca4899bdec260ddc92a7bc98ceaa8da0ee5bd，源码TaskToolInput含optional effort、required parent_step_id与model/effort分别覆盖resume描述，用户指南也已同步。相同旧systemhead哈希不表示工具schema已热更新。工具定义由运行二进制及其tool runtime提供，安装文件更新不能替换已运行进程。

父已读调查报告/有界脚本并亲跑两个原request投影，保存 `/tmp/grow-effort-parent-proof.json`；原调查在 `/tmp/grow-effort-investigation-01a1a9c0/`。正式发布包为 `/tmp/grow-261-published-42138009783/grow`，已通过来源证明、codesign、version/help，版本grow2.6.1(deaca48)。后续仅允许隔离GROW_HOME/project+loopback mock确认实际provider工具声明，无真实provider/MCP/hooks或用户进程修改，无Cargo。

验收分别核旧请求事实与正式发布包actualschema。若发布包已有正确参数，完成维护记录即可，无新增业务实体/回退/测试框架。父检查实际projection/命令与进程清理、同步简短用户文档并执行strict/archive；不增加独立review。
