# T4 / U2 / U3 成本门槛调查

本次只测量并决定是否值得实施，不修改业务行为。基于当前 dirty 工作树（HEAD `bbd2c4dfa`，不能把它当作干净提交），未改 C3、backlog 或其他 agent 的文件。结论：**三个候选均可按当前证据关闭，不新增缓存或替换算法**。这不是宣称任意规模下零成本：T4/U2 合成大规模仍有增长，U3 的被审计排序已不在生产入口。

## 方法与边界

- 环境：Apple M4 Pro / arm64 / macOS 27.0.1，rustc 1.99.1；仓库 dev profile `opt-level=0`，未新建 release/bench target。数字是本机 debug 当前代码成本，不是 release 吞吐、provider 端到端时延或优化前后收益。
- 每项预热10次，9个原始样本；每样本为batch总时长/次数，median取第5项。U2在N<512时batch50，否则10；T4 kernel在N<512时batch100，否则10；T4真实actor/registry及U3均batch100。使用 `black_box`，输出对象的释放在计时内，fixture创建、schema字节序列化在计时外。
- 每个cargo进程独立临时 `GROW_HOME`，`RUST_MIN_STACK=16777216`、`--locked`、`--test-threads=1`，未读真实session或调用provider/MCP外部服务。机器没有CPU pin或关闭后台负载，保留全部原始samples，不把波动当收益。
- 首轮命令：`cargo test --locked -p shell -p pager --lib cost_gate -- --ignored --nocapture --test-threads=1`，只运行3个ignored探针，Pager 2 passed、Shell 1 passed。第二轮只补T4首个registry复制边界，命令见后。
- 下表时间均为 **µs/次**。`derive`仅filter+owned显示数据；`paint`通过真实生产绘制函数进入，包括其派生、group/entry映射及buffer绘制，**不包括ACP获取、磁盘、ratatui terminal flush或整个AppView画面**。T4“actor边界”包括真实 async prepare+ToolSpec构造，不包括完整模型请求。

## T4：当前工具清单成本小，不建立immutable snapshot

真实路径为 `SessionActor::prepare_tool_definitions_inner` → ToolBridge/registry的 `tool_definitions_builtins_only`（owned深复制）→当前Timeline/Goal/Behavior动态过滤→`turn_base_tool_specs`（再次clone并转换）。探针使用官方 `workspace_grow_build_toolset()` 与既有actor test support创建Normal actor，未用手写schema代替产品schema。

补测首个registry边界：动态过滤前30工具，schema JSON **25,151 B**、description **13,876 B**，合计39,027 B；真实 `ToolBridge::tool_definitions_builtins_only` owned复制median **190.675 µs**（含该async wrapper，registry本身同步锁/过滤/clone）。同进程完整actor边界median **368.368 µs**，与首轮362.625µs接近。

当前最终manifest 27工具，schema紧凑JSON字节21,187，description UTF-8字节11,774，合计32,961。**这些是序列化内容字节，不是allocator实际clone分配量**。首轮真实actor prepare+spec边界median **362.625 µs/step**；其中最终27定义的Vec深复制median142.456，ToolSpec构造143.224。两个kernel独立运行，不把它们相加冒充同一时间样本。

| N | schema JSON B | description B | definitions owned clone | turn base spec |
| --- | --- | --- | --- | --- |
| 27 | 21,187 | 11,774 | 142.456 | 143.224 |
| 128 | 101,436 | 56,090 | 671.278 | 666.053 |
| 512 | 401,801 | 223,198 | 2616.225 | 2655.262 |
| 2048 | 1,607,883 | 892,965 | 10967.683 | 10947.875 |

128/512/2048行是循环复用真实27个definitions构造的**纯kernel压力负载**，不是当前产品实际工具数或MCP用户分布。当前builtin manifest明确排除`__` MCP动态工具；不能把多MCP数量直接代入这条曲线。当前preset的整个prepare+spec边界约0.36ms；没有证据表明为它引入snapshot所有者、版本失效和共享schema类型能改善当前可观察瓶颈。原需求关心的Recall完成/rewind可见性、turn捕获Behavior、Goal和MCP更新时效都已有动态owner，保持现状。相关权威包括`extension-runtime`的current connection/tool-list刷新契约及本路径既有compaction/Behavior回归；本次未修改这些契约，也未声称性能探针验证了全部行为。

## U2：默认列表30项的派生成本不足以支持缓存

实际入口：Welcome的`render_session_picker`；Agent/child详情的`draw_active_modal` SessionPicker分支。两者都用同一个 `effective_filter_query`/`filter_session_entries`/`build_session_entry_data_at`，Agent绘制还构造grouped map。探针直接调用这两个生产paint入口，80×24、grouped=true、5个repo，包含ID/title/cwd/model/host/branch/时间字段；collapsed和全部expanded分别测量。`filter-and-derive`查询为`session`（命中全部），派生使用固定now，paint沿真实FrameStamp。

当前Pager `FetchSessionList`和Dashboard fetch显式`limit:30`，Shell unified list的`DEFAULT_LIMIT`也是30；没有发现当前UI默认100的入口。100作为要求的额外规模保留，128/512/2048是超出当前默认页规模的合成压力。未扫描真实用户session，不伪造实际数量分布。

| N | 展开 | derive | filter+derive | Welcome picker paint | Agent modal paint |
| --- | --- | --- | --- | --- | --- |
| 30 | collapsed | 6.672 | 8.578 | 501.154 | 215.517 |
| 30 | expanded | 109.347 | 111.515 | 581.894 | 339.890 |
| 100 | collapsed | 21.148 | 27.431 | 584.609 | 268.377 |
| 100 | expanded | 378.104 | 388.072 | 1019.591 | 786.457 |
| 128 | collapsed | 27.426 | 35.116 | 592.365 | 284.895 |
| 128 | expanded | 490.046 | 510.202 | 1226.640 | 929.155 |
| 512 | collapsed | 107.546 | 135.779 | 778.579 | 482.458 |
| 512 | expanded | 1899.421 | 1913.183 | 3467.571 | 3183.938 |
| 2048 | collapsed | 416.475 | 525.675 | 1508.662 | 1205.717 |
| 2048 | expanded | 7495.846 | 7646.846 | 12717.562 | 12375.058 |

默认30项collapsed派生仅6.672µs，paint总计约0.501/0.216ms；30项全展开约0.582/0.340ms。100项全展开约1.020/0.786ms。2048项全展开确实达到12.7/12.4ms，但当前默认请求不返回这个规模；不能用超页负载推导当前帧已经有瓶颈。为省默认30项的少量派生而维护query/width/expansion/content-arrival/repo/time/selection多源失效，会新增状态和一致性风险，没有足够收益证据。本轮关闭U2，保留完整派生入口。没有实施优化，因此不声称做了新旧selection/map差分；现有`client-surfaces` picker最新fetch、visible owner和history内容搜索契约保持。

## U3：top-8全排序不在当前生产Dashboard路径

`build_rows`调用`build_local_rows(include_subagents=true)`，先过滤workflow、全量stable sort、取前8并生成more。全repo引用核对显示它只被row单测和dispatch/tests调用；生产`render_dashboard`、`next_wall_clock_deadline`、`dashboard_neighbor_row`、`dispatch_dashboard_select`全部经`build_rows_with_roster[_at]`传false，**在排序前continue**。Atlas scoped structural search定位build_rows，再用源码及全repo引用核对；不把局部Atlas空调用结果当全库证明。

探针使用真实SubagentInfo构造（每7个一个workflow、1/3 finished、带相等时间戳），N=8/32/128/512/2048。kernel复制当前收集和稳定排序比较器；`nonproduction-full-tree-build`调用真实build_rows；`production-overview-build`调用真实build_rows_with_roster。三者不能混称整个Dashboard paint。

| 总children N（含workflow） | collect+stable sort top8 kernel | 非生产full-tree build | 生产overview build |
| --- | --- | --- | --- |
| 8 | 0.447 | 15.105 | 2.368 |
| 32 | 3.225 | 23.536 | 2.829 |
| 128 | 15.928 | 37.034 | 4.307 |
| 512 | 90.449 | 119.152 | 11.430 |
| 2048 | 334.225 | 390.280 | 42.461 |

生产行构造依然会读取child活动/计数，因此不是零成本；但它没有该候选所说的full stable sort。2048 children时kernel334µs不应归给生产overview的42µs，更不能归给整帧。这个排序候选是已失效的热点归因，本轮按入口事实关闭，既不优化测试专用路径，也不顺带删除公开helper或另造top-k算法。

## 原始samples（首轮，保持工具输出数值）

```text
COST U3 collect-full-stable-sort-top8-kernel n=8 us=[0.46291000000000004, 0.4425, 0.45208, 0.45083, 0.44708, 0.44875, 0.44416000000000005, 0.44292000000000004, 0.44583999999999996] median=0.447
COST U3 nonproduction-full-tree-build n=8 us=[15.33791, 14.888340000000001, 14.853340000000001, 14.8425, 14.907919999999999, 15.11667, 15.27209, 15.5825, 15.105419999999999] median=15.105
COST U3 production-overview-build n=8 us=[2.36708, 2.36125, 2.36542, 2.3625, 2.36833, 2.37125, 2.375, 2.3933400000000002, 2.3875] median=2.368
COST U3 collect-full-stable-sort-top8-kernel n=32 us=[3.0991699999999995, 3.2025, 3.11, 3.41542, 3.26875, 3.2245800000000004, 3.17416, 3.25792, 3.25417] median=3.225
COST U3 nonproduction-full-tree-build n=32 us=[23.559160000000002, 23.53625, 24.19333, 22.455, 23.56625, 22.384169999999997, 23.01, 23.61375, 23.23959] median=23.536
COST U3 production-overview-build n=32 us=[2.83541, 2.95875, 2.82875, 2.82625, 2.82125, 2.8333299999999997, 2.9220800000000002, 2.82375, 2.81083] median=2.829
COST U3 collect-full-stable-sort-top8-kernel n=128 us=[15.94375, 16.02875, 15.84125, 15.927919999999999, 16.04209, 16.04625, 15.885, 15.9125, 15.852920000000001] median=15.928
COST U3 nonproduction-full-tree-build n=128 us=[37.81875, 37.590830000000004, 37.43667, 37.452090000000005, 36.58792, 35.54708, 35.37542, 37.034169999999996, 36.575] median=37.034
COST U3 production-overview-build n=128 us=[4.3025, 4.31708, 4.30333, 4.31583, 4.30708, 4.305420000000001, 4.3116699999999994, 4.30583, 4.3675] median=4.307
COST U3 collect-full-stable-sort-top8-kernel n=512 us=[91.45916000000001, 90.81792, 90.89667, 90.44875, 89.8925, 134.33292, 87.65375, 88.56958999999999, 88.18334] median=90.449
COST U3 nonproduction-full-tree-build n=512 us=[115.8, 118.10457999999998, 114.945, 119.4925, 120.5975, 119.15208999999999, 120.16542, 119.83208, 117.65834] median=119.152
COST U3 production-overview-build n=512 us=[11.12666, 11.485, 11.43042, 11.37125, 11.57459, 11.110829999999998, 11.55, 11.511670000000002, 11.00709] median=11.430
COST U3 collect-full-stable-sort-top8-kernel n=2048 us=[341.02625, 334.66375, 335.78042, 329.2725, 328.10582999999997, 328.43625, 332.64042, 334.225, 335.35208] median=334.225
COST U3 nonproduction-full-tree-build n=2048 us=[390.36625, 390.27958, 397.40208, 391.06166999999994, 391.34458, 387.9779199999999, 389.52833, 389.65959000000004, 388.92] median=390.280
COST U3 production-overview-build n=2048 us=[42.47540999999999, 42.61, 42.50375, 42.45792, 42.405, 42.4075, 42.46084, 42.84125, 41.995419999999996] median=42.461
COST U2 derive/collapsed n=30 us=[6.629160000000001, 6.67416, 6.66, 6.675840000000001, 6.6725, 6.671659999999999, 6.651660000000001, 6.676660000000001, 6.66832] median=6.672
COST U2 filter-and-derive/collapsed n=30 us=[8.62082, 8.60332, 8.59166, 8.5875, 8.57832, 8.566659999999999, 8.5525, 8.54918, 8.55668] median=8.578
COST U2 welcome-picker-paint/collapsed n=30 us=[501.15416000000005, 501.1025, 499.89084, 500.9, 499.86834000000005, 504.4925, 504.0975, 507.80916, 506.29] median=501.154
COST U2 agent-modal-paint/collapsed n=30 us=[214.56082000000004, 214.43, 214.26416, 216.78584000000004, 216.40584, 215.5875, 215.4925, 215.80166, 215.51666] median=215.517
COST U2 derive/expanded n=30 us=[109.46166000000001, 109.37916000000001, 109.3475, 109.25584, 109.18418, 109.04331999999998, 109.21665999999999, 110.02915999999999, 110.20916000000001] median=109.347
COST U2 filter-and-derive/expanded n=30 us=[112.18666000000002, 112.1175, 111.515, 111.64916000000001, 111.66584, 111.02416000000001, 111.0525, 111.13668, 111.435] median=111.515
COST U2 welcome-picker-paint/expanded n=30 us=[584.5333400000001, 581.8941599999999, 582.1475, 582.9066799999999, 584.1916600000001, 580.48918, 578.68668, 579.51584, 579.93582] median=581.894
COST U2 agent-modal-paint/expanded n=30 us=[343.68582000000004, 338.65831999999995, 339.19916000000006, 344.0625, 338.155, 339.5475, 339.89, 342.24165999999997, 340.95083999999997] median=339.890
COST U2 derive/collapsed n=100 us=[21.588340000000002, 21.3375, 21.15418, 21.135, 21.2175, 21.105, 21.14832, 21.109160000000003, 21.10666] median=21.148
COST U2 filter-and-derive/collapsed n=100 us=[27.81582, 27.339159999999996, 27.398339999999997, 27.26084, 27.3025, 27.7275, 27.70834, 27.430819999999997, 27.5225] median=27.431
COST U2 welcome-picker-paint/collapsed n=100 us=[574.0375, 577.99916, 597.77332, 593.5816599999999, 591.17, 591.36832, 583.21416, 584.6091600000001, 578.67084] median=584.609
COST U2 agent-modal-paint/collapsed n=100 us=[266.45668, 267.22668, 273.82084000000003, 268.35166000000004, 268.04668000000004, 270.9025, 269.04166000000004, 269.3425, 268.3775] median=268.377
COST U2 derive/expanded n=100 us=[381.03, 378.10416, 379.6175, 378.65666, 376.76415999999995, 377.27832, 377.8225, 377.8925, 380.02666] median=378.104
COST U2 filter-and-derive/expanded n=100 us=[388.0725, 390.60416, 388.86415999999997, 382.47, 383.9025, 380.1225, 384.1, 392.70167999999995, 398.2725] median=388.072
COST U2 welcome-picker-paint/expanded n=100 us=[1032.03, 1019.5908400000001, 1022.93168, 1042.7075, 1016.4866799999999, 999.0866599999999, 998.7308400000001, 1000.52584, 1055.66668] median=1019.591
COST U2 agent-modal-paint/expanded n=100 us=[758.74, 758.9016799999999, 787.20084, 795.1225, 817.2275, 786.45668, 786.7325, 784.7891799999999, 778.8716800000001] median=786.457
COST U2 derive/collapsed n=128 us=[27.425819999999998, 26.955, 27.6675, 28.91166, 29.2, 27.66334, 26.66668, 26.74418, 26.8975] median=27.426
COST U2 filter-and-derive/collapsed n=128 us=[36.445, 35.205, 34.765, 34.6625, 34.26916, 34.255, 35.91416, 35.13666, 35.11582] median=35.116
COST U2 welcome-picker-paint/collapsed n=128 us=[597.155, 590.97916, 589.3525, 601.42918, 590.66084, 591.58168, 592.9525, 592.365, 602.04084] median=592.365
COST U2 agent-modal-paint/collapsed n=128 us=[284.895, 284.32084, 284.24834, 284.3875, 285.13581999999997, 286.57332, 284.90834, 284.34834, 289.01084] median=284.895
COST U2 derive/expanded n=128 us=[501.8875, 491.74415999999997, 500.21666, 481.38084, 484.82332, 487.365, 490.04583999999994, 489.05084000000005, 492.65918] median=490.046
COST U2 filter-and-derive/expanded n=128 us=[502.31084000000004, 513.6675, 512.01084, 512.5441800000001, 510.20165999999995, 505.66666000000004, 507.2725, 494.80832000000004, 518.08668] median=510.202
COST U2 welcome-picker-paint/expanded n=128 us=[1249.87916, 1236.63, 1244.48, 1226.64, 1228.6275, 1199.09834, 1223.9433199999999, 1216.62916, 1199.71418] median=1226.640
COST U2 agent-modal-paint/expanded n=128 us=[932.6008399999998, 929.155, 928.49166, 929.24334, 933.5091600000001, 928.50084, 930.7775, 925.945, 926.6058400000001] median=929.155
COST U2 derive/collapsed n=512 us=[107.5459, 107.6292, 108.35829999999999, 107.3708, 107.2666, 108.39169999999999, 107.30410000000002, 107.48330000000001, 107.5667] median=107.546
COST U2 filter-and-derive/collapsed n=512 us=[136.3458, 136.5833, 135.5291, 135.925, 135.5875, 135.7375, 135.7792, 135.60829999999999, 136.8458] median=135.779
COST U2 welcome-picker-paint/collapsed n=512 us=[774.65, 775.5042000000001, 776.1416999999999, 778.4042, 778.5791, 789.25, 788.5582999999999, 790.3583000000001, 785.4709] median=778.579
COST U2 agent-modal-paint/collapsed n=512 us=[482.9958, 481.625, 481.37919999999997, 482.475, 483.6834, 482.45830000000007, 472.5708000000001, 473.59580000000005, 483.64579999999995] median=482.458
COST U2 derive/expanded n=512 us=[1924.1457999999998, 1912.7917000000002, 1891.2541, 1883.6083999999998, 1899.4207999999999, 1876.2375, 1907.8417000000002, 1901.1208000000001, 1894.8916000000002] median=1899.421
COST U2 filter-and-derive/expanded n=512 us=[1946.925, 1946.0207999999998, 1917.2292000000002, 1913.1833000000001, 1909.0333999999998, 1900.3625, 1919.875, 1902.0834, 1893.425] median=1913.183
COST U2 welcome-picker-paint/expanded n=512 us=[3494.8707999999997, 3477.2667, 3474.6542, 3466.6832999999997, 3467.5708, 3467.7083999999995, 3426.2290999999996, 3415.5208, 3421.829199999999] median=3467.571
COST U2 agent-modal-paint/expanded n=512 us=[3182.4458, 3180.9916000000003, 3186.9, 3199.3708, 3195.3625, 3171.8999999999996, 3173.6708000000003, 3183.9375000000005, 3190.1333] median=3183.938
COST U2 derive/collapsed n=2048 us=[415.8125, 416.34580000000005, 417.9833, 418.0792, 422.625, 416.475, 414.8917, 417.45, 415.9125] median=416.475
COST U2 filter-and-derive/collapsed n=2048 us=[525.675, 525.8625, 526.0292, 526.6875, 525.6, 524.4875, 524.9042, 525.2042, 526.5459] median=525.675
COST U2 welcome-picker-paint/collapsed n=2048 us=[1508.6625, 1505.5416, 1508.2208, 1504.9042, 1514.6082999999999, 1511.4916, 1518.8083000000001, 1509.7791, 1507.0333] median=1508.662
COST U2 agent-modal-paint/collapsed n=2048 us=[1204.3708000000001, 1223.8709000000001, 1224.1, 1207.3625, 1200.7291, 1210.3125, 1204.125, 1205.7166, 1200.9875] median=1205.717
COST U2 derive/expanded n=2048 us=[7466.0834, 7587.8958, 7692.4042, 7643.6875, 7707.2292, 7490.0208, 7494.6375, 7476.4708, 7495.8458] median=7495.846
COST U2 filter-and-derive/expanded n=2048 us=[7647.7625, 7626.0, 7646.8458, 7611.3542, 7624.7709, 7630.695899999999, 7733.1292, 7757.133400000001, 7668.7625] median=7646.846
COST U2 welcome-picker-paint/expanded n=2048 us=[12807.7958, 12592.779100000002, 12570.641699999998, 12555.9583, 12648.699999999999, 12838.554199999999, 12800.9875, 12781.795800000002, 12717.562499999998] median=12717.562
COST U2 agent-modal-paint/expanded n=2048 us=[12331.8458, 12310.1584, 12375.0583, 12339.579099999999, 12433.8791, 13078.725, 12842.841699999999, 12437.325, 12302.4875] median=12375.058
COST T4 actual tools=27 schema_desc_bytes=(21187, 11774)
COST T4 actual-prepare-and-spec us=[408.17625, 355.73292000000004, 361.01584, 362.625, 369.2075, 361.12, 359.18417, 371.16458, 362.7075] median=362.625
COST T4 kernel n=27 schema_desc_bytes=(21187, 11774)
COST T4 definitions-owned-clone n=27 us=[139.62584, 143.34625, 140.95458, 142.45583, 143.66208999999998, 146.55125, 143.87125, 142.3325, 141.12958] median=142.456
COST T4 turn-base-spec-construction n=27 us=[141.995, 142.84542000000002, 149.33833, 146.90958, 146.87125, 143.86667, 143.22375, 142.64, 140.91665999999998] median=143.224
COST T4 kernel n=128 schema_desc_bytes=(101436, 56090)
COST T4 definitions-owned-clone n=128 us=[681.86625, 677.27458, 685.05625, 671.27792, 672.93667, 666.63625, 668.97875, 664.60542, 662.8029100000001] median=671.278
COST T4 turn-base-spec-construction n=128 us=[665.08542, 674.82042, 671.5783299999999, 673.94667, 664.32083, 665.36375, 666.0525, 671.42417, 663.05042] median=666.053
COST T4 kernel n=512 schema_desc_bytes=(401801, 223198)
COST T4 definitions-owned-clone n=512 us=[2608.3125, 2613.8583, 2613.1417, 2619.4458999999997, 2639.1584, 2616.225, 2642.1584000000003, 2606.3084, 2651.7542000000003] median=2616.225
COST T4 turn-base-spec-construction n=512 us=[2653.8042, 2657.4667, 2655.2625, 2646.1792, 2598.1375, 2639.2083, 2667.8707999999997, 2678.5166999999997, 2752.6792] median=2655.262
COST T4 kernel n=2048 schema_desc_bytes=(1607883, 892965)
COST T4 definitions-owned-clone n=2048 us=[10710.35, 10582.9542, 10559.975, 10441.4875, 11140.3334, 11035.9542, 11120.6, 10967.6833, 11566.3875] median=10967.683
COST T4 turn-base-spec-construction n=2048 us=[10953.8334, 11084.8542, 10989.1333, 11250.2583, 10947.875, 10833.05, 10774.4459, 10821.0125, 10939.7709] median=10947.875
```

## 可复现探针源码与恢复

临时修改只是在以下三个既有文件末尾追加一段唯一marker包围的 `include!("<临时probe源绝对路径>");`。源码在本节完整保留；复跑时分别保存到临时`.rs`，append include，运行上述定向命令，最后只删除相同后缀。不能reset/checkout既有dirty文件。三个探针源各定义唯一`#[cfg(test)]`模块，测试显式`#[ignore]`。

### crates/codegen/shell/src/session/actor/tests/rewind_cross_compaction_tests.rs

追加前SHA256：`401d337038ab8692e9c69e15961b490e5510777726eeb7697487bb11f150d32f`。

```rust

#[cfg(test)]
mod cost_gate_manifest_probe {
 use super::*;
 use std::{hint::black_box,time::Instant};
 fn measure(label:&str,n:usize,mut f:impl FnMut()) {
   for _ in 0..10 {f();}
   let batch=if n>=512 {10}else{100};let mut samples=Vec::new();
   for _ in 0..9 {let t=Instant::now();for _ in 0..batch {f();}samples.push(t.elapsed().as_secs_f64()*1e6/batch as f64);}
   let mut sorted=samples.clone();sorted.sort_by(f64::total_cmp);
   println!("COST T4 {label} n={n} us={samples:?} median={:.3}",sorted[4]);
 }
 #[tokio::test(flavor="current_thread")] #[ignore]
 async fn cost_gate_manifest() {
  tokio::task::LocalSet::new().run_until(async {
   let (gateway_tx,_gateway_rx)=tokio::sync::mpsc::unbounded_channel();
   let (persistence_tx,_persistence_rx)=tokio::sync::mpsc::unbounded_channel();
   let actor=create_test_actor(0,200_000,80,gateway_tx,persistence_tx).await;
   actor.agent.replace(test_agent_with_tools(agent::config::workspace_grow_build_toolset().tools).await);
   let defs=actor.prepare_tool_definitions_inner().await;
   let bytes=|v:&[sampling_types::ToolDefinition]| -> (usize,usize) {
     (v.iter().map(|d|serde_json::to_vec(&d.function.parameters).unwrap().len()).sum(),v.iter().map(|d|d.function.description.as_ref().map_or(0,String::len)).sum())
   };
   println!("COST T4 actual tools={} schema_desc_bytes={:?}",defs.len(),bytes(&defs));
   let bridge=actor.agent.borrow().tool_bridge().clone();
   let raw=bridge.tool_definitions_builtins_only().await;
   println!("COST T4 raw-builtin-before-dynamic-filter tools={} schema_desc_bytes={:?}",raw.len(),bytes(&raw));
   let mut raw_samples=Vec::new();
   for _ in 0..10 {black_box(bridge.tool_definitions_builtins_only().await);}
   for _ in 0..9 {let t=Instant::now();for _ in 0..100 {black_box(bridge.tool_definitions_builtins_only().await);}raw_samples.push(t.elapsed().as_secs_f64()*1e6/100.0);}
   let mut ordered=raw_samples.clone();ordered.sort_by(f64::total_cmp);
   println!("COST T4 actual-registry-owned-clone us={raw_samples:?} median={:.3}",ordered[4]);

   let mut samples=Vec::new();
   for _ in 0..10 {black_box(actor.prepare_tool_definitions_inner().await);}
   for _ in 0..9 {let t=Instant::now();for _ in 0..100 {let d=actor.prepare_tool_definitions_inner().await;black_box(actor.turn_base_tool_specs(&d));}samples.push(t.elapsed().as_secs_f64()*1e6/100.0);}
   let mut sorted=samples.clone();sorted.sort_by(f64::total_cmp);println!("COST T4 actual-prepare-and-spec us={samples:?} median={:.3}",sorted[4]);
   for n in [defs.len(),128,512,2048] {
    let inflated:Vec<_>=(0..n).map(|i|defs[i%defs.len()].clone()).collect();
    println!("COST T4 kernel n={n} schema_desc_bytes={:?}",bytes(&inflated));
    measure("definitions-owned-clone",n,||{black_box(inflated.clone());});
    measure("turn-base-spec-construction",n,||{black_box(actor.turn_base_tool_specs(&inflated));});
   }
  }).await;
 }
}

```

### crates/codegen/pager/src/views/session_picker.rs

追加前SHA256：`393e54c30f081d23d70026cd45269dbdf01a4964f794dec8eea862d5cfa252bd`。

```rust

#[cfg(test)]
mod cost_gate_picker_probe {
 use super::*;
 use std::{hint::black_box,time::Instant};
 use crate::views::{picker::PickerState,modal::ActiveModal,modal_window::ModalWindowState};
 use ratatui::{buffer::Buffer,layout::Rect};
 fn measure(label:&str,n:usize,mut f:impl FnMut()) {
   for _ in 0..10 { f(); }
   let batch=if n>=512 {10} else {50};
   let mut samples=Vec::new();
   for _ in 0..9 {let t=Instant::now();for _ in 0..batch {f();}samples.push(t.elapsed().as_secs_f64()*1e6/batch as f64);}
   let mut sorted=samples.clone();sorted.sort_by(f64::total_cmp);
   println!("COST U2 {label} n={n} us={samples:?} median={:.3}",sorted[4]);
 }
 #[test] #[ignore]
 fn cost_gate_picker() {
   let now=chrono::DateTime::from_timestamp(1793664000,0).unwrap();
   let area=Rect::new(0,0,80,24);
   for n in [30,100,128,512,2048] {
    let entries:Vec<_>=(0..n).map(|i| SessionPickerEntry {
      id:format!("session-{i:04}"),summary:format!("Session {i}: implement task planning and session recovery"),
      updated_at:now-chrono::Duration::hours(i as i64),created_at:now-chrono::Duration::days(i as i64+1),
      cwd:format!("/workspace/repository-{}",i%5),hostname:Some("test-host".into()),model_id:Some("test-model".into()),
      num_messages:12,last_active_at:None,branch:Some("main".into()),repo_name:format!("repository-{}",i%5),worktree_label:None,card_detail:None,
    }).collect();
    for expanded in [false,true] {
      let mut state=PickerState::default();
      if expanded {state.expanded.extend(0..n);}
      let label=if expanded {"expanded"}else{"collapsed"};
      measure(&format!("derive/{label}"),n,|| {
        let indices=filter_session_entries(Some(&entries),"");
        black_box(build_session_entry_data_at(&entries,&indices,&state,80,now));
      });
      measure(&format!("filter-and-derive/{label}"),n,|| {
        let indices=filter_session_entries(Some(&entries),"session");
        black_box(build_session_entry_data_at(&entries,&indices,&state,80,now));
      });
      let mut buf=Buffer::empty(area);
      measure(&format!("welcome-picker-paint/{label}"),n,|| {
        black_box(crate::views::welcome::render_session_picker(area,&mut buf,&crate::theme::Theme::default(),
          &mut crate::views::welcome::SessionPickerRenderCtx {state:&mut state,sessions:Some(&entries),cwd:std::path::Path::new("/workspace/repository-0"),loading:false,pending_hint:None,shortcuts_area:None,content_results:None,content_loading:false,entries_query:None,frame:crate::motion::FrameStamp::default(),grouped:true}));
      });
      let mut agent=crate::app::agent_view::test_agent_view(Some("cost-gate"),"/workspace/repository-0".into());
      agent.active_modal=Some(ActiveModal::SessionPicker {state,entries:Some(entries.clone()),loading:false,previous_palette:None,window:ModalWindowState::new(),content_results:None,content_loading:false,deep_search_seq:0,entries_query:None,pending_delete:None});
      measure(&format!("agent-modal-paint/{label}"),n,|| {
        agent.draw_active_modal(area,&mut buf,crate::theme::Theme::default(),false,crate::motion::FrameStamp::default());black_box(&buf);
      });
    }
   }
 }
}

```

### crates/codegen/pager/src/views/dashboard/row.rs

追加前SHA256：`2b8b9082dfa42cad3b6e05e3b7a63913083d6c6829093ac7edd3af4186942822`。

```rust

#[cfg(test)]
mod cost_gate_child_probe {
 use super::*;use std::{sync::Arc,hint::black_box,time::Instant};
    fn make_subagent(child_id: &str, finished: bool, status: Option<&str>) -> SubagentInfo {
        let now = Instant::now();
        SubagentInfo {
            subagent_id: Arc::from(format!("sa-{child_id}")),
            child_session_id: Arc::from(child_id),
            description: Arc::from("test task"),
            subagent_type: Arc::from("explore"),
            model: None,
            reasoning_effort: None,
            context_source: None,
            resumed_from: None,
            capability_mode: None,
            permission_mode: None,
            effective_permission_mode: None,
            workflow_run_id: None,
            context_normalized: false,
            child_updates_replayed: false,
            parent_prompt_id: None,
            started_at: now,
            last_progress_at: now,
            finished,
            status: status.map(Arc::from),
            error: None,
            duration_ms: None,
            tool_calls: None,
            turns: None,
            turn_count: None,
            tool_call_count: None,
            tokens_used: None,
            context_window_tokens: None,
            context_usage_pct: None,
            tools_used: Vec::new(),
            error_count: None,
            activity_label: None,
            is_background: false,
            pending_kill: false,
            kill_requested_at: None,
            scrollback_entry_id: None,
            prompt: None,
            child_cwd: None,
            worktree_path: None,
        }
    }

 fn measure(label:&str,n:usize,mut f:impl FnMut()) {
   for _ in 0..10 {f();}let mut samples=Vec::new();
   for _ in 0..9 {let t=Instant::now();for _ in 0..100 {f();}samples.push(t.elapsed().as_secs_f64()*1e6/100.0);}
   let mut sorted=samples.clone();sorted.sort_by(f64::total_cmp);println!("COST U3 {label} n={n} us={samples:?} median={:.3}",sorted[4]);
 }
 #[test] #[ignore]
 fn cost_gate_children() {
  for n in [8,32,128,512,2048] {
   let mut agent=crate::app::agent_view::test_fixtures::make_agent();agent.display_name=Some("cost gate".into());
   let base=Instant::now();
   for i in 0..n {let mut child=make_subagent(&format!("child-{i}"),i%3==0,None);child.started_at=base-std::time::Duration::from_millis(((i*17)%31)as u64);if i%7==0 {child.workflow_run_id=Some(Arc::from("wf"));}agent.session.subagent_sessions.insert(format!("child-{i}"),child);}
   let mut agents=IndexMap::new();agents.insert(AgentId(0),agent);
   measure("collect-full-stable-sort-top8-kernel",n,|| {
    let mut children:Vec<_>=agents[&AgentId(0)].session.subagent_sessions.values().filter(|i|i.workflow_run_id.is_none()).collect();
    children.sort_by(|a,b| match (!a.finished,!b.finished) {(true,false)=>std::cmp::Ordering::Less,(false,true)=>std::cmp::Ordering::Greater,_=>b.started_at.cmp(&a.started_at)});black_box(&children[..children.len().min(8)]);
   });
   measure("nonproduction-full-tree-build",n,|| {black_box(build_rows(&agents,&Default::default(),&[],None,super::super::state::Grouping::Open,&super::super::state::Filter::None,None));});
   measure("production-overview-build",n,|| {black_box(build_rows_with_roster(&agents,&Default::default(),&[],None,super::super::state::Grouping::Open,&super::super::state::Filter::None,None,&[]));});
  }
 }
}

```

## T4首轮复制边界补测

命令：`cargo test --locked -p shell --lib cost_gate_manifest -- --ignored --nocapture --test-threads=1`，独立临时GROW_HOME，1 passed。probe源码已包含此补测；第二轮原始samples如下，不用第二轮较小的个别数值宣称优化收益。

```text
COST T4 actual tools=27 schema_desc_bytes=(21187, 11774)
COST T4 raw-builtin-before-dynamic-filter tools=30 schema_desc_bytes=(25151, 13876)
COST T4 actual-registry-owned-clone us=[203.79667, 189.52291999999997, 190.02125, 189.02458, 190.67458, 186.50041, 195.21457999999998, 192.9725, 192.98] median=190.675
COST T4 actual-prepare-and-spec us=[367.64333000000005, 368.53667, 363.80875, 368.23, 368.64583, 368.62583, 368.62375, 368.36792, 368.36] median=368.368
COST T4 kernel n=27 schema_desc_bytes=(21187, 11774)
COST T4 definitions-owned-clone n=27 us=[143.31625, 142.75082999999998, 142.72041, 142.87209000000001, 143.57625, 142.58458000000002, 142.84915999999998, 142.65082999999998, 142.99667] median=142.849
COST T4 turn-base-spec-construction n=27 us=[144.05333, 144.415, 143.31542, 143.52417, 143.43416, 143.58416, 143.41, 143.065, 143.40041] median=143.434
COST T4 kernel n=128 schema_desc_bytes=(101436, 56090)
COST T4 definitions-owned-clone n=128 us=[677.1666700000001, 675.21292, 680.1725, 674.51375, 675.8075, 693.41292, 677.44625, 676.16292, 675.80625] median=676.163
COST T4 turn-base-spec-construction n=128 us=[681.46083, 684.1204100000001, 682.09958, 681.7425, 684.14292, 683.29458, 683.575, 684.85083, 683.95375] median=683.575
COST T4 kernel n=512 schema_desc_bytes=(401801, 223198)
COST T4 definitions-owned-clone n=512 us=[2663.0, 2662.0667000000003, 2665.6584, 2665.025, 2671.5459, 2666.6167, 2675.2666, 2665.7583, 2675.175] median=2665.758
COST T4 turn-base-spec-construction n=512 us=[2725.1459, 2720.575, 2719.3542, 2716.2459, 2714.7167, 2719.15, 2719.5792, 2718.8792000000003, 2691.7209000000003] median=2719.150
COST T4 kernel n=2048 schema_desc_bytes=(1607883, 892965)
COST T4 definitions-owned-clone n=2048 us=[11074.0833, 10980.9375, 10770.7125, 10746.241600000001, 10892.1666, 10813.0833, 10803.4083, 10750.0625, 10747.1334] median=10803.408
COST T4 turn-base-spec-construction n=2048 us=[10736.5375, 10660.6458, 10833.5667, 10795.4917, 10764.779100000002, 10799.7542, 10799.3583, 10733.0333, 11061.7958] median=10795.492
```

## 临时修改清理确认

三个include后缀已按精确字节删除，恢复后SHA256全部与追加前相同（上文记录各原始hash）。没有reset、checkout或覆盖预先dirty内容。所有探针源码保留在本文件，临时源/原始日志目录及本轮setup/report脚本已清理；未留下业务/测试代码改动，唯一持久交付是本cost-gates.md。共享target沿用原目录，未新造独立Rust target或release产物；本轮仅定向编译和运行3+1项测量，没有重全量cargo。
