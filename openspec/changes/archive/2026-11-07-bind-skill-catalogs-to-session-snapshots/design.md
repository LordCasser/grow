# Design

## 已核证据和职责

父亲核`extensions/skills.rs`、`mvp_agent/acp_agent.rs`、Pager effects及SkillManager；独立调查b18a交叉检索实际请求/DTO/effect调用链。当前目录扫描支持disabled和未激活conditional条目，模型目录另由session SkillManager进行资格控制。UI管理目录不能换成只有模型已激活项，也不能把返回UI列表写成runtime baseline。准备证据见`../complete-nondeferred-backlog/skills-scope-preparation.md`。

`SessionHandle::plugins_list`现把成功None、send失败和reply丢失都转None；既有command payload可表达正确None，缺口只在wrapper。唯一已定位仓内调用者为extensions/plugins.rs。MvpAgent有精确primary lookup（等待同id load）与含child的control lookup；当前Skills生产modal只需要primary。无session请求为磁盘检查，不需要猜primary。

## 请求解析契约

在现六种catalog/config方法list、toggle、add、remove、reset、config上接受可选`sessionId`。值的类型必须正确、显式字符串必须非空。`refresh-baseline {}`继续是单独的全局广播，不套此resolver。

- 有sessionId：用`session_handle_waiting_for_load`解析精确primary；missing、load失败、child-only或已退休owner明确错误，不按cwd找另一个session、不fallbacklatest/native-only。cwd为非null字符串时与sessionId互斥并invalid_params；cwd:null按未指定处理。通过同一handle取得实际`tool_context.cwd`及`PluginsList` owner回执，冻结本次扫描的snapshot。
- 无sessionId：保留每个既有方法的cwd规则。list仍需明确字符串cwd；add/remove/toggle/reset/config保既有默认点cwd及null语义，reset/config非对象或cwd类型错误仍在配置变动前拒绝。按所得cwd与既有trust/effective plugins配置调用纯`build_for_cwd(..., session_plugin_dirs=&[], trust)`；保process显式`--plugin-dir`，不含任意session roots、不发布sharedlatest。
- 全局配置mutations先解析/确认上下文，再按现事务写入；后续发现失败仍报告“已保存但reload失败”，不伪回滚。disabled仍按全局dedupkey，不在此次变成每session开关。

“owner有效”限定在同id owner确认快照这一准入时点；扫描使用冻结副本，不承诺owner在整个阻塞扫描期间持续resident。UI session+epoch guard决定过期回包能否应用。无新增owner lease、generation表或快照公共DTO。

## 最小实现边界

Shell owner复用现request DTO、bounded discovery worker和PluginsList command。把现`plugins_list`窄改为能区分成功None与通信失败的Result，并适配plugins/list已有caller传播通信错误；它的unknown-session历史fallback政策不在此修复中扩大。私有contextresolver的具体表示由coder选择，不新增公开错误类型或平行注册表。对异步扫描budget仍沿既有worker，ctx构造含同步registry发现应在相同执行边界，不把扫描移回async请求线程。

Pager owner只把现effect已有sessionId发到线，并在发起时捕获session_binding_epoch；两者一路到SkillsListLoaded/SkillsToggleDone。在改skills_data、error、pending_action、pending_entry_index前验证同绑定，success/error都校验。A→B→A也必须丢旧回包。复用SwitchAgentCatalogLoaded的session/epoch模式；同绑定请求新旧排序不新增nonce框架。所有effect生产处，包括统一extensions tab fetch、new/load后抓取、手动Reload、PluginsChanged、toggle均捕获；toggle后原refresh-baseline广播保留。

AC-06另有直接配置事件交付和custom watcher roots，不在此变更中修。AC-03a另有LSP bootstrap，复用session快照事实但分别交付。

## Coder Task Specs与Review Plan

1. **Shell返回正确上下文目录**：只交上述request解析、exact primary快照读取、cwd-only只读构建和必要plugins_list错误适配。通过真实MvpAgent→ACP list/toggle A/B不同cwd及samecwd/differentpluginDirs、sharedlatest污染反控；无session扫描不含session roots；合法None正常native-only、owner send/rx失败明确error；invalid/missing上下文先于mutation。已有cwd默认/nullable、raw保存后reload失败、discovery预算回归保持。先有效oldred再最小fix/newgreen。
2. **Pager Skills回包保请求归属**：依Shell已冻结wire `{sessionId}`、无cwd。只交effect和TaskResult身份/epoch贯穿及资格校验；实际effect抓包证明线身份，旧success/error/toggle回包A→B和A→B→A不改新view目录或pending状态。无sessionmodal仍先创建session再抓，原global refresh不变。

父分别读实际diff/入口证据，接口到位后做一次真实UI list/toggle→对应session目录/模型精确load的跨层验证。外部ACP可达但无仓内sender的add/remove/reset/config仍按契约测试，不以未定位仓外客户端证明可删除它们；公开Rust返回类型变化列为必要接口影响，不重建兼容层。strict/archive和开发说明在实现验证后完成。当前只有方案，不勾实施任务。
