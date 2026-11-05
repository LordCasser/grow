# 审计修复的整体架构

2026-11-05用户将本轮收束为发布前里程碑并要求停止新审查。以下全范围矩阵保留为已完成的架构调查和后续输入，仅[里程碑记录](milestone.md)中接受的功能及当时已在途的必要闭环进入当前收尾；未启动项不再实施或派review。原“全部backlog修复”不再是本次change的关闭前提，也不能因此把未修项勾成通过。

## 目标和当前依据

当前57包的既有边界足以承载修复：CLI/Pager/ACP负责产品入口和投影；Shell SessionActor负责会话接纳、Step、资源采用与编排；ChatState/Timeline拥有持久会话事实；Sampler拥有provider任务；MCP拥有连接和发送；Workspace与具体数据crate拥有磁盘动作；Config拥有原始配置读改写；Workflow journal拥有操作重放。历史全版本报告是定位证据，本次派发前复核当前调用方和工作树，不能当作已修或已测。

当前不增加统一调度器、全局事务管理器、通用generation/lease框架、第二份canonical状态。优化依据是数据/权限正确性优先，其后恢复一致性，再减少派生状态和重复逻辑，最后根据测量收缩成本。不能证明抽象意义的全局最优；每个结构选择接受跨入口反例检验，若反例推翻owner则先修订契约再继续依赖项。

## 共同决策

| 边界 | 已有owner负责的保证 | 下游应消费的结果 |
| --- | --- | --- |
| 输入和队列 | Timeline Input生命周期、原始请求、route与替换关系 | Shell恢复/重试查询同一事实；不能再次按当前foreground重判历史请求 |
| 会话插件 | 每会话构造/采用的PluginRegistry快照，包括None | Hooks/LSP/Skills使用同一会话来源；进程latest不能代替指定session |
| 外部调用 | Sampler任务身份/MCP精确client、SDK注册连接身份 | await后的清理或发送仅影响原身份；同名或同RequestId不表示同一实例 |
| 配置与安装 | 原始TOML读改写锁/atomic publish，安装registry与文件变化各自真实结果 | CLI/ACP只在承诺边界成功后报告；部分完成必须保留可诊断证据 |
| 磁盘动作 | fast-worktree删除范围；hunk actor对实际内容版本和成功提交负责 | 指针不是删除权；缓存不是磁盘版本；失败不先更新统计/turn投影 |
| 恢复journal | operation结果不可变，读前缀需确认持久屏障 | Workflow engine/展示复用同一fold，不以可读替代已durable |
| 传输恢复 | CLI pending RPC id和已确认session集合 | reconnect只重放仍归该客户端、未关闭的真实session |
| 资源与诊断 | PtySession、test phase、crash record各自有终结/发布owner | 输出EOF不代替child结算，直接child退出不证明独立后代已回收，文本保存失败不消费raw |

## 实施映射及依赖

本表是架构关系，实际运行/写入所有权和交付接受由Grow任务跟踪。独立项不因同文件而合并，同一规则的真实调用方适配随该项交付。

| Backlog项 | 实现owner与一项结果 | 前置/关联及里程碑 |
| --- | --- | --- |
| WS-03 | fast-worktree只删除checkout和验证属于它的linked registration | 可独立先行；Workspace force fallback/失败清理同调用链核对，真实linked/separate/forged三对照 |
| WS-06b | hunk action成功后再提交统计和turn索引，失败保持 | 先行，给06a新增冲突Err提供正确失败边界；批次按文件已成功部分保留真实事实，不做全项目rollback |
| WS-06a | hunk reject以实际磁盘内容/存在性验证展示版本，冲突不覆盖 | 在06b冻结后接手同actions；单/file/turn/all共用条件，不能只改single |
| WS-01 | codebase-graph归约保留Modified→Removed的删除 | 独立；增量索引最终与磁盘一致，含defs/refs/file metadata和rename |
| WS-02 | ClientFS完整内容与digest来自同一代内容 | 不以提高时间精度假装修复；先确定去memo或既有身份/读取方案，stat/read完整公共操作验收 |
| WS-04 | MemoryStorage仓库key保留host/port、同仓库SSH/HTTPS等价 | 独立；既有持久目录不能静默跨仓继承，原资料保留，确切转换语义派发前定 |
| WS-05 | envrc路径作为进程参数，合法特殊字符不被求值 | 独立；沿现trusted wrapper，不重写信任系统，进程期限另按实际证据处理 |
| Session F3 | stable retry先返回canonical input身份/生命周期 | 先确定共享事实查询，再落实F1/F2消费者，避免重复fold |
| Session F1 | 保留挂起Turn的steer route并恰好续跑消费 | 依赖原Timeline/runtime已有保留语义，不能放宽valid_input_reroute；与F3共享查询 |
| Session F2 | durable edit继承FIFO位置和原队列运行身份 | replacement映射须支持多次编辑、消费和cold，和F3共同集成 |
| Session F5 | journal相同operation完成结果不可被异值重写 | 与F4同journal按顺序交付；先统一live/loader/projector fold |
| Session F4 | 恢复读取的journal prefix经文件/目录持久确认再继续 | journal storage接口→Shell实现→engine恢复联动，保留不确定与原operation |
| AC-01 | plugins/CTA config writer纳入Settings既有raw mutation锁和atomic publish | plugin持久错误AC08需要这一可信基础；同步UI/CLI调用形态不能blocking_lock堵runtime |
| AC-02 | Some→None清空旧plugin Hook贡献 | 和AC03/06共用session snapshot采用语义；不删除Agent/config Hook |
| AC-03a | LSP bootstrap使用本次session registry | 保留现父子共享LSP的显式语义；不同cwd/同cwd不同pluginDirs反例 |
| AC-03b | Skills API明确session来源，并与模型catalog一致 | 跨Shell/Pager共享请求契约先定；cwd不足以选择session，不加猜测fallback |
| AC-06 | Skills配置事件到session reload owner并重配watch roots | 在AC03b作用域确认后交付leader/inprocess两个入口；不重建第二SkillManager |
| AC-04 | Agent规则从一致物理cwd/checkout建祖先链 | 已有discovery用户dirty保留；alias内部子目录必须包含root且不纳外部祖先 |
| AC-05 | Agent walker检测祖先目录环，合法namespace稳定 | 独立，不重写Skill walker；当前遍历边界与非环link策略先定 |
| AC-07 | plugin update采用已有UpdateResult写回registry | AC08之前同application写面顺序接手；CLI/ACP共享已有apply函数 |
| AC-08 install | 安装关键I/O错误传播，部分发布保留可重试事实 | 依赖AC01/07已定持久界面，单独任务不捆绑uninstall |
| AC-08 uninstall | 删除失败不虚报完整卸载/丢失记录 | 与install同文件顺序；删除/registry save/rollback各失败真实可诊断 |
| SAMPLER-1 | 完成/panic按任务身份只清自身登记 | 与MCP都遵循精确身份但不共享抽象，公开RequestId替换语义保持 |
| MCP-F3 | standard/SDK声明在副作用前同一canonical重名验证 | 与F2入口字段共享，先确定统一source契约再分别实施 |
| MCP-F2 | SDK reverse路由保留注册连接owner，driver不替代 | registration→invoker→leader跨模块接口先定，断线不回退driver |
| MCP-F1 | 初始化后发送前复核原client/disabled/祖先资格 | model/direct共用发送owner，撤销后旧新client tools/call都0；不自动重放 |
| MCP-D1 | 主规范旧retry场景与no-replay统一 | 文档独立change，可先行，不为旧文字改变正确实现 |
| PB-01 | forward真实入口将close纳入replay状态 | PB02同CLI文件顺序；closedsession不在reconnect集合 |
| PB-02 | pending new按JSON-RPC id归属 | PB01冻结后；逆序new/error/load交织集成 |
| PB-03 | PtySession child终态驱动stop/timeout/linger和server关闭 | session公共终态契约先定，CLI只接graceful关闭，不再自行猜EOF |
| PB-04 | exact runner如实确认owned descendants回收或unconfirmed | 与PTY都属生命周期但不同owner；对setsid+DEVNULL保持显式不确定，不能扩全局kill权 |
| PB-07 | crash durable report成功才消费raw | PB05同恢复owner，先定结果/失败语义；写失败保留证据 |
| PB-05 | crash capture每进程文件身份与恢复claim | 在PB07提交边界上扩多进程；信号handler不分配/锁/普通I/O框架 |
| PB-06 | crash历史地址按原module身份/相对地址或明确unknown | 依赖PB05记录身份稳定后改format/symbolicate，旧raw不能套当前ASLR映射 |
| ACP-1 | 先验证慢handler/producer容量风险及真实调用约束 | 实验后定队列/handler预算与过载语义；不能只加semaphore死锁reverse control |
| MD-1 | 先分开测parse与输出clone/分配成本 | 按真实收益选择借用/共享表示，保持batch输出一致，不先大改render API |
| Vendor docs | 同步maintained sqlite-vec/nix-ohos实际维护说明 | 纯文档，不删除维护patch或升级依赖 |
| Agent实体/announcements | 确认public seam用途，收缩无必要依赖 | 内部无调用不证明外部可删；announcements转config可独立、无新paths层 |
| child SessionEnd Hook | Result关闭Timeline前完成其应有Hook生命周期 | 与Result/retirement唯一owner统筹，normal/cancel/cold三入口；不重开封闭Timeline |
| 迟到结果现场 | 新源码已修且隔离Goal全链通过 | 旧live host未采用、Receipt Unknown仍保留；不得用源码修复伪造现场终态 |

## 调查后补充的约束

索引WS01的当前归约/应用链已由父再次核对：CoalescedEvents仍把Created或Modified后来的Removed从map删除；actor的apply_coalesced只对残留Removed调用真实ScopeGraphIndex.remove_file，后者负责definitions/references/file metadata。归约器不持有批次前索引状态，Created也不能证明该路径尚无旧索引（例如替换/rename目的地已存在）。因此后续修复应保留批次最终Removed，包括Created→Modified→Removed、Removed→Created→Removed及rename目的地再删除；删除未知路径沿现remove_file幂等处理。保留Created→Removed对照的含义是最终索引为空，不继续把空map内部表示当外部契约。无需新增初始存在性状态或改Shell事件协议；实施时从已索引真实Rust文件经IndexManagerHandle::send_events的FileEventBatch和同步snapshot查询验证defs/refs/metadata及rename，不能只测map。

WS02父已补读ClientFsStatReq/ReadFileReq真实WorkspaceOp、shared memo所有者及`stream_hash_and_range`。现memo仅以path+size+mtime_ms复用hash；命中read路径会另开文件只读range，因而不同代内容可配旧digest。已有stream helper一次打开文件，逐块同时算full SHA256并提取range，返回实际streamed size，足以作为后续最小修复落点。提高mtime精度或再加inode仍不能证明未改内容；在没有可信内容代际的现契约下，候选决定为移除该memo，stat/read都消费现完整stream路径，read的hash/range/size来自同一遍读取。代价是重复完整hash I/O，不预建watcher generation协议。该决定还未实施或实测；并发原地writer下普通读取不提供文件系统快照，不能把同遍stream一致性宣称为某瞬间全文件原子快照。后续公共WorkspaceOp回归需先缓存原内容再构造同大小同毫秒不同纳秒、保持mtime改写与atomic replacement，逐项验证返回内容/全hash/range/size及原root confinement/read cap。

输入F1/F2/F3当前接口仍有缺口：Timeline已有input_state（含RouteSuperseded）和runtime_state，Shell现有平行扫描漏supersedes、retry intent受foreground影响、restore无条件Steer→FIFO。先确定只读canonical生命周期接口，再分别改F1和F2；F2恢复顺序从supersedes根槽位派生，queue row/run identity与revision request identity分开。F1可复用现runtime保留事实，不必等待所有RPC返回改造。显式reorder只改内存的契约尚需核定，不当作已解决。

插件快照方面，父已读实际session_plugin_registry构造和LSP另读process snapshot、Some-only Hook清理、Pager effect发送cwd:"."。AC02/03a已有接口足够；AC03b先冻结必填sessionId、以目标handle的cwd和plugin snapshot生成目录，禁止cwd猜session或借sharedlatest。管理目录还需核对conditional skills投影，不能直接把slash_skills当全目录；AC06复用采用完成ACK及watcher roots生命周期。注册/验证子Agent还读sharedsnapshot的分支待单独核对，未承诺其已闭合。

Sampler父已读actor/state/handle/commands，旧CancelOnDrop除正常join清理外也按同RequestId误取消替代提交。修复应覆盖同一提交的完成、panic与drop取消：可使用JoinSet任务Id（tracked期间唯一）和每提交CancellationToken；public cancel(RequestId)仍指当前登记，collect future drop只取消自己的token。保持现cooperative结算，不加全局nonce/lease。MCP身份语义与此不同，仍各自owner。

F3后续消费者边界也已定位：`actor/input_admission.rs::find_existing_prompt_input`在比较同prompt时按调用方当前计算的intent拒绝，`existing_input_state`另扫事件但漏AdmissionResolved.supersedes隐含的RouteSuperseded dismissal；`actor/prompt_queue.rs::queue_input`用当前foreground决定Prompt/Followup，导致同请求跨idle/running重试不稳定。Timeline已有`input_state`返回原intent/payload_ref/权威state（Handled在canonical fold归Consumed），而Handle目前只暴露pending_allowed_inputs与runtime_state。后续应先通过现query通道暴露该canonical查询，再使重试按原始请求route/payload核对而非当前调度intent；不据此预建新输入登记。具体RPC状态返回、缺失事实与并发首次提交的对接仍须实施前定约。F1恢复落点为同file restore_pending_human_inputs的无条件Steer→Fifo分支；F2使用该函数的Submitted顺序和payload revision prompt_id，不能只改queue meta显示ID就声称恢复顺序/运行身份已保留。

MCP调查指出额外需核定的前提：is_current_client只覆盖local map/disabled、不覆盖祖先资格；SDK注册连接身份不能信任客户端可写的leaderClientId缺省字段；standard/SDK分别合法不代表合并合法。MCP-F1/F2/F3实施前父须亲查祖先资格、leader注入/重连规则和source merge，当前只记录候选，不按建议直接落新公共接口。

## 分阶段验证

每次派发先具备明确owner、源版本、写scope和可证伪验收。首阶段优先WS03/WS06/AC01及MCP撤销、挂起恢复，独立ready任务可并行；同一file/公共契约依赖未满足时顺序。保持最多足以整合的在途实现，优先review和吸收结果。Cargo窗口串行或使用可证独立target，避免半编源码和机器争用；只读调查可并行。

里程碑按真实链路而非目录打包：A数据动作公共API→磁盘/投影；B配置写→session snapshot采用→Hook/LSP/Skills；C输入接纳→编辑/steer→挂起cold恢复→相同identity重试；D SDK声明/owner→握手→撤销/替换→一次tool call；E CLI/reconnect/PTY/诊断恢复。高风险项均独立luna review，父检查关键前提、失败语义和共享接口。每项先用Tier1/2/3适用回归闭环，到里程碑再运行真实产品入口，避免逐项重跑全仓。

清理backlog必须逐项有对应修复/验证归档；推翻原finding要保留反例和证据，不删失败场景。平台未运行和实验未证明不计关闭。其他明确暂缓方向继续保持原启动条件，不能借本次用户授权预建RemoteControl/多轮elicitation等产品。
