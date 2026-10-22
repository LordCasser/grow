# P1 release 验证与收益

## 结论与范围

P0 先独立归档，再按其约81%的 loading batch Markdown 终态开销启动 P1。P1 在现有 renderer / wrap cache 上分开原文完结与派生展示，不改 Timeline、持久化、协议、历史顺序或 ready 屏障；没有 SQLite、分页、后台全量恢复或提前放开输入。

实测支持保留 P1：现代文本长会话在默认关闭诊断的真实 CLI/PTY 中，512/2048 轮末屏中位数均降低约59%；慢输出2048轮降低约55%。128轮持平，首条历史出现没有一致改善。不能将局部约6倍的 handler 提升外推为整体6倍。

## 构建、样本与测量边界

实际编译器是 Rust1.99.1 Homebrew，普通 release / CLI 默认 features、jemalloc+sandbox，locked/offline、jobs4、incremental0，专用 `target/resume-attribution-release`。仓库 toolchain 文件写1.93.1，但本 shell PATH 选择 Homebrew；P0 留存的 build/run context 同样是1.99.1，已纠正其 verification prose 的错误标签。没有混合两个编译器作 A/B。

Before 为已验证的 P0 CLI SHA256 `f6ecedff6b19d9693071d16900aeee25f3541f2318cd0a53d1e0f30956b64f0d`，after 为 P1 `dcd4aebfa016de6d3828d49e8e8a2cc1081669da277eca3abbf0ede2bb615785`。默认-off与详细-log两组身份一致；unit、PTY、RPC和Markdown executable身份另存。最终CLI包含page-edge settle；最后的source修改只在 cfg(test)布局测试中明确固定实际边界，unit重新构建。`source-final-audit.json`确认最终source/config相对最终unit构建无漂移；Shell/Timeline/持久化及third_party未改。

每组3对 AB/BA、fresh process / warmed executable+fixture file cache；新 executable 先预热。warmup与所有exploratory、失败记录保留但不计入正式表。正式测量不与本任务的编译或正确性检查并发；机器非隔离环境，外部活动不受控制。n=3是本机对照证据，不是延迟保证。固定现代 admitted 历史：每轮8×4096B正文，实际投影折叠为每轮user+assistant，PTY额外一条末屏sentinel合并进最后assistant。所有15对详细观测的消息类型、文本字节、finish数量和完整source字节均相同，见 `source-count-checks.json`。

`history_visible`是最后sentinel在PTY模拟屏幕可见；首个sequence marker不保证是turn1；PTY receipt与屏幕观察使用各自已标记时钟。默认-off的 `ready_ms`明确 unavailable，不能写成0或以末屏代替ready。详细日志的ready值是完整finalize完成后的观察，prompt-stable另外包含约500ms固定观察窗口。

## 默认关闭诊断的真实 TUI

中位数 [min,max]，单位ms，n=3；原始日志与逐run汇总在 `default-paired/`。

| 样本 | Before 末屏 | After 末屏 | 中位数变化 |
| --- | ---: | ---: | ---: |
|128轮|165.6 [158.8,239.7]|164.5 [162.6,239.8]|基本持平|
|512轮|420.0 [408.9,461.3]|173.1 [171.4,192.5]|−58.8%|
|2048轮|1384.2 [1379.8,1391.0]|560.9 [552.3,570.4]|−59.5%|
|40ms慢writer，512轮|542.3 [539.8,591.9]|279.8 [263.7,350.1]|−48.4%|
|40ms慢writer，2048轮|1487.1 [1476.6,1756.8]|668.9 [642.5,706.5]|−55.0%|

慢输出表取真实progress-observation终点；同一次随后的PTY终点汇总分别542.6→280.0、1487.5→669.2ms，边界差异不混用。512 after全部只有一次真实推进，但从可见Loading到末屏为120.7–140.2ms；不为制造进度增加延迟。2048 after仍有2–3次推进，最大内容间隔115.5–144.2ms；before有9次推进，间隔138.4–355.6ms。更少帧是加载更快的结果。单帧的gap=0不证明连续滚动无停顿；较长样本仍保留严格多次推进门槛。

慢2048首条标记before496.0 [411.9,501.9]→after525.2 [409.9,590.9]ms，没有一致首屏收益；不能宣称每个阶段都改善。普通输入echo中位数在128/512/2048 after分别4.2/4.9/4.0ms，慢writer约50ms，仍受40ms人工writer延迟影响。

## 归因与剩余成本

独立无绘制控制（完整source/order断言，3对）:

|轮数|decode/project before→after|handler before→after|AgentView finish_reload before→after|
|---|---:|---:|---:|
|128|14.897→15.227|59.783→9.951|0.453→0.080|
|512|57.019→59.214|226.354→37.852|0.485→0.110|
|2048|235.024→239.232|918.504→150.090|0.823→0.426|

单位ms。decode未优化，after512/2048分别保留174.991/423.602ms慢样本；不能用中位数掩盖它们。此控制不运行完整SessionLoaded dispatcher，不能用其finish_reload替代整个就绪收尾。

详细-log真实TUI的loading batch合计512为236.494→47.034ms，2048为962.232→191.917ms。对应eager Markdown finish为191.207/772.661ms，after source-finish约微秒级；派生解析没有消失。完整pre-key trace中，after每run均有一次终态物化，512/2048约0.4ms，通常在finalize后的首次展示需求中发生，因此被loading-prefix截点排除。`presentation-demand-checks.json`同时保存prefix与完整pre-key计数。未完结的当前可见消息仍可在加载帧中正常解析、换行；后续滚动/复制/搜索会为新需求物化历史。

详细-log普通2048末屏1402.7→669.6ms、完整finalize观察1725.9→938.7ms；512为414.4→175.4、511.7→295.7ms。详细普通2048首次sequence可见419.6→430.4ms，同样没有首屏提速结论。不要将nested phases相加。`session.load_session_replay`下降包括UI背压减少，不是新的存储算法/RPC对照；backend保持原样。独立retained-backend oracle只作128/2048全量历史正确性检查，其时长不作A/B。

完整finalize 2048仍约246.9→241.8ms，根Timeline/descendant恢复校验重读未动。总体仍需O(历史字节)的完整读取、校验、原文处理和交付；减少的是未被使用的全量Markdown派生工作，变为随可见/复制/搜索需求执行。100ms绘制节奏、40ms人工writer等待、startup/调度及根校验使短样本不一定改善。

## 正确性与对抗审查

最终scope总计 **2423 passed / 0 remaining failed**：Pager1750（ACP354、tracker165、load64、fork49、event-loop91、scrollback1021、block-viewer6），retained且源码未变的writer/image171，Markdown502。3项manual ignored不计作通过；唯一debug-assert-only release exclusion与P0完全相同，并保留其原始失败，未改should_panic或生产断言。`checks.json`和逐组日志可逐项核对。

新增17项重点校验：原文normalizer尾部、UTF-8/tab/CRLF、表格/列表/引用/code/math/URL；pretty/raw复制、links/source map、first-wrap、nested读、raw/width、repeat finish、pending clone、late live/deferred append；PNG/JPEG引用的finish时刻探测、缺失/损坏及首次展示前删除/改写、内部图片占位符完整/不完整/late suffix；大小写/容器/多个/字符实体Mermaid和按钮行；Thinking历史时长；cache冷热无关估算；实际deferred历史的尾部精确布局、滚动上/下、resize、eager sticky anchor对照与独立layout总高oracle。图片测试保留的是完成时引用/尺寸观察与展示结构，不声称锁定外部文件的像素内容。

既有32 Grow/7 ACP families的每物理prefix对照、root/child顺序及80/120列完整渲染、所有工具/权限/状态、图片恢复和持续更新仍通过ACP全组。现代128/2048 retained-backend逐正文完整oracle通过。18个no-draw实际ignored执行逐正文/顺序/数量核对；两种诊断模式各3对普通128/512/2048及慢512/2048、Ctrl+D退出后重恢复pair均通过，普通loading输入被忽略且zero inference；实际follow-up turn before/after均通过，完整历史仅出现一次。

独立源码对抗审查修正了实体Mermaid漏检和近似高度下page-edge选择；独立数据复核确认CLI身份、median/range及阶段边界。编译时新增测试的macro歧义、旧慢512帧数断言、两次错误resize测试前提均保留失败证据，修正后重跑；没有删除失败场景或放松大样本进度要求。详见 `adversarial-review.md`。

## 代价与未实施项

原文含Mermaid字面候选或任意ampersand的消息保守地保持结束时解析（包括普通query或代码`&&`的误报），图片探测仍同步。因此文本fixture的收益不能外推到图表/图片/这类候选密集历史。第一次滚动、pretty复制或全历史搜索会承担此前尚未请求的解析；clone旧实现也可能提前并重复构建view。未建立新缓存框架或为了成绩改持久化同步级别；未声称内存/存储下降，因为本项没有独立相应测量。

根Timeline已校验child ownership的边界内复用是下一候选，须另建change，不能按UI无child跳过校验。本项到此停止；未做就绪与历史分页分离。验证、归档和专用target清理结果见 completion.json。

## 收尾结果

归档前全量strict17/17；归档后主规范16/16、归档687/687，均0失败，见对应validate日志。P1已独立归档，新增client-surfaces契约已合入主规范，开发者说明同步更新。archive命令中的6/7只剩归档/验证自身的行政checkbox，执行后已勾选；所有实现与验证门槛在归档前完成。

最终源码与二进制身份留证后，删除专用 `target/resume-attribution-release`（含P0/P1对照executables及全部中间产物），删除前分配占用10.565GiB；共享target和真实sessions保留，已确认专用路径不存在。原始release日志、casts、归因与失败记录作为验证证据保留。
