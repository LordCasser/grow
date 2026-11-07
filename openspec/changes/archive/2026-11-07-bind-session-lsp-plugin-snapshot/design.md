# Design

## 现有职责与依据

Shell bootstrap拥有新会话ToolContext及其LSP backend构造；Agent PluginRegistry拥有按cwd、session roots、trust的发现结果。`refresh_and_build_for_cwd`不把会话独有roots发布为进程latest，因此共享snapshot不能表达当前会话资格。bootstrap现成的 `session_plugin_registry` 正是解析Agent和交给SessionActor的已许可快照；LSP段从同值派生active plugins即可，不需新的registry、缓存或第二次reload。

父亲核了构造的两处读取、原tool_ctx.lsp.is_none gate、sourced loader与folder_trust filter、inspect只读同loader对照。loader本身保user/project/plugin precedence，不承担从错会话快照中再识别资格。旧审计的静态发送链仍在当前源码成立，尚未跑真实双会话进程反例，不把旧报告写成现场复现。

## 决策与边界

唯一目标是本次需要新建LSP backend时，从本次session快照取得file/inline配置和plugin names。已有继承backend沿原Arc共享，不重新创建、关闭或替换。None/empty-active即无plugin contribution，不能回退shared latest；user/允许project配置仍正常参与。保lsp_tools_enabled gate、launchcwd、ProcessScope与ensure_started_background时序，生命周期完全沿原LspManager。

不同时修AC-03bSkills API/AC-06watcher，也不让plugin refresh触发LSP重启。那些是独立责任/契约。替代方案是bootstrap先把本会话registry发布全局再读，仍有跨会话race并泄露session roots，否决；新增LSPregistry或每调用重新discovery均重复已得到的资格，不采用。

## Coder Task Spec / Review Plan

一个Shell bootstrap coder负责一个会话LSP隔离行为。最小改既有LSPsnapshot取值，测试用真实MvpAgent公共new-session入口，不能只把两个不同registry直接塞给loader后称bootstrap通过。临时HOME/GROW_HOME/project/roots，用可控stdio JSON-RPC LSP stub记录server identity/进程启动事件，初始化握手barrier后核同名server在A/B各执行对应来源。配置含file-based和manifest inline两个入口；同cwd不同session pluginDirs验证cwd不能替代session身份。shared A→创建B且B为空/disabled-active时不得启动A命令，B自身可允许user/project对照；已继承backend分支不覆盖。

保留旧红source日志和新绿，同名source与实际启动归属可证；不要仅靠tool_ctx.lsp_server_names（同名无法判来源）。进程按本测试owner回收，不按全局名称杀。父核最终diff/公共入口/信任和继承对照；相关loader/会话回归通过后同步开发说明、backlog、strict/archive。平台进程验证只报告实际运行平台，Windows现场不可得不作保证。

验收入口补充（父2026-11-07核实际文件）：`shell/tests/test_trusted_local_plugin_refresh_e2e.rs`现非ignored测试直接调用refresh/discovery，不能当作本次MvpAgent bootstrap证据；复用其中plugin树/启用配置形状即可。实际公开ACP new_session+LocalSet/duplex可参照`test_advisory_question_admission.rs`或registry churn测试，无需发模型请求。LSP stdio消息形状见Tools现`implementations/lsp/tests/mock_servers.rs`，无需为了Shell测试将该私有helper改成公共API。隔离配置用`[features].lsp_tools=true`，保GROW_LSP_TOOLS优先级；session pluginDirs通过每个NewSessionRequest._meta传入，不向共享registry发布以绕过反例。新进程的initialize握手、来源marker、PID、进程cwd与rootUri均要记录。父核`tools/lsp/client.rs::start_stdio`未设`Command::current_dir`，实际进程cwd继承宿主；`InitializeParams.root_uri`才从会话传入的effective_root派生，因此不把两者混为同一保证，不要求本修复改变进程cwd。验证以来源命令＋初始化rootUri识别本次会话，记录实际os.getcwd作对照。退出沿session原ProcessScope/LspManager所有权。
