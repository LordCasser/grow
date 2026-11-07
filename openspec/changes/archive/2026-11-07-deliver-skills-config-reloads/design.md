# Config delivery and discovery roots

## 依据

父核 ConfigReloader::Skills(old/new compare)、Shell app.rs Skills只日志、Pager spawn.rs config_reload_request wildcard及两边watcher启动。既有 grow/internal/reload_skills → reload_skills_all_sessions → 每session reload_skills_from_disk 已拥有fresh配置/插件快照与slash发布。SkillsFileWatcher::start根据cwd/workspace user/paths构造完整计划，并由调用方持有其取消/Drop owner。没有必要新增catalog、协议或全局监听表。

## 决定与共享契约

配置更新总是沿原内部reload入口投递，无论paths是否变化，disabled/ignore也必须刷新。leader通过已有ACP internal request格式，in-process通过相同session_admin handle，不能只给UI广播或只更新startup AgentConfig。该投递是既有异步reload准入，不伪称每session瞬时完成；实际测试等模型目录与slash广告同一owner确认。

各运行入口的既有watcher task持有一个用于最新paths更新的watch channel（仅调度，不是配置权威）。ConfigUpdate::Skills把最新paths交该task；真实reload仍读fresh有效配置。path变更用现SkillsFileWatcher::start(cwd,workspace_user,paths)在blocking pool构造替换；成功后采用新watcher并在blocking上下文释放旧owner，不在async core阻塞Drop。失败保旧watcher并明确日志，不发布假新监听；runtime取消时，登记中的新watcher及旧watcher都沿既有finish_blocking_registration/取消路径回收。只disabled/ignore变化不必重建同paths watcher。

新配置可能在首次watcher构建期间到达，最新watch值必须在登记完成后消费，不能因task尚未ready丢掉。重建期间又有path变更，则保留最新pending值并继续采用；不要求预建generation框架。旧源已排队的事件可触发一次幂等reload，但移除root后不再持有其native watch。未知启动失败不能自动变成永久不支持以后paths更新；caller已有None处理需核清：若根目录当时不存在，后续有效配置必须仍能创建watcher。

不改ConfigReloader有效配置与失败解析政策，不把坏config当空成功广播。全局/项目watcher的resident所有权和workflow目录刷新继续保持；新custom root的创建/修改需真实文件事件验证，不能仅通过人工构造ConfigUpdate证明。

## macOS discovery registration: confirmed native blockage

最终真实回归在 `Started` 后十二秒仍未 `Active`；隔离 cwd 对照仍失败。对本次测试 child 采样，Skills 与 ProjectDiscovery 的注册均阻塞在 notify 8.2.0 `FSEventStreamStart → register_with_server → mach_msg`，见 `/tmp/grow-ac06-native-child-92817.sample.txt`。首次 native start 本身也阻塞，故批登记减少重复 start 不能作为充分修复，亦不延长验证期限。

共享 discovery watcher 的 macOS backend 改用 notify 已有 metadata polling，其它平台保留 RecommendedWatcher。沿用本模块既有 MacConfigWatcher 避免 FSEvents 反复 stop/start 的平台选择方式，不新增 watcher service、fallback 状态机或公开接口。轮询间隔沿用 discovery debounce，关闭内容比较，避免读取 custom root 任意大文件；创建、修改、删除、新目录挂载继续经过原分类和 reload owner。mtime 变化采用 notify 原语，不承诺保留相同文件元数据的不可观察编辑。

本增量只落在 Shell config/watcher 的共享 discovery owner，不修改通用 fsnotify 文件索引 backend。验收仍用原九项真实路径/文件/reload 和两项相邻生命周期回归、原十二秒期限；保留全部 native 失败证据。若轮询仍不能满足场景，保留失败，不删断言。

## Coder Task Specs

1. **leader运行入口**：只交Shell agent/app.rs leader ConfigUpdate::Skills到internal reload和该leader discovery watcher paths替换、必要测试/自己的验证记录。不得改Pager in-process或Skills API。Tier2/3：真实leader配置文件改disabled/ignore，不碰skill目录，现session精确调用资格+slash广告跟随；新增external root后创建/改skill可见，移除后watch释放；启动注册race、取消和失败构造保owner。已有UI toggle不计此项。父核真实internal请求与Session结果。
2. **in-process运行入口**：只交Pager acp/spawn.rs同等行为，依上面相同现有接口/契约；不用共享新manager。真实spawn_shell→config file→session调用资格/advertisement及paths后续事件覆盖，已有config_reload_request单测可做小回归但不代替入口。取消登记和gateway early exit原tests保持。写面不与leader冲突时可并行，Cargo串行。

若实验证明需要修改共享watcher构造接口或已有reload owner行为，先向父报告证据，冻结新接口后再交给对应所有者，不能自行由两边猜接口。父做跨入口一致性审查与实际产品里程碑；最终主spec/docs/backlog/archive由父统一维护。未跑平台明确限制，不以macOS文件事件代表Windows。
