# Verification

状态：旧planner反例、同快照保护修复、ChatState及Shell真实入口定向回归、独立review与当前CLI集成均已完成；原Timeline guard和消费语义保持。

## Mechanism and baseline

父与luna分别核对Timeline::unconsumed_skill_load_indices、Compaction/ToolResultPrune guard、materialize/plan/fit与单summary replacement。未消费状态基于branch-selected canonical response；Sideband不消费skill。无现场Timeline样本，不将推定入口或source session写成已复现。

coder先使用修复前已编译chat-state生产rlib（09:09产物）链接轻量rustc驱动 `/tmp/grow-unconsumed-compaction-baseline.rs`，不启动Cargo或接触正常存储。fixture含真实形状的Model SkillLoad及reasoning/backend+双tool batch，后续有较新prompt和raw assistant但无canonical消费。单项断言原本应仅选旧安全prefix(1,3)，旧planner实际输出：

```text
1 test
selectedcount=9 selected=1..=9 includes_skill=true
```

测试exit101，选区含受保护skill，证明旧planner没有该保护输入。此轻量反例用于planner差异，不替代ChatState真实消费/provenance和Shell provider入口验证。

## ChatState verification

Rust1.93.1、RUST_MIN_STACK=16777216，每条命令独立临时GROW_HOME。coder运行 `cargo test --locked -p chat-state --lib <filter> -- --test-threads=1`；父亲自读取两份实际日志，结果：

| Filter | Actual result |
| --- | --- |
| partial_compaction_ | 13 passed，0 failed，606 filtered out |
| actor::tests::materialization_pins_skill_until_canonical_response_not_raw_or_summary (exact) | 1 passed，0 failed，618 filtered out |
| unconsumed | 8 passed，0 failed，611 filtered out |
| fit_compaction_range | 4 passed，0 failed，615 filtered out |

筛选有重叠，不将总次数当不同场景数量。13项含既有prefix/tail/身份/完整工具组与新增Model/User pins、exact缺失、source不足、prompt内split；1项真实materialization区分raw/summary与canonical消费；8项保留原guard/settle/replay；4项保留fit不拆组/不吞未完成证据。首轮新增测试漏None参数导致编译失败，补齐后最终通过，未改production契约来迎合测试。

生产候选已稳定：同snapshot保护IDs由原Timeline fold投影；planner完整保留Model call/result及响应组和User同prompt输入，预算fit只缩小safe plan。Timeline guard/终止settle实现未改。额外发现Shell两个冷物化构造需要同一已验证Timeline投影真实pins，已各增一行，Timeline getter公开只读；无新构造框架/状态/存储I/O。main与child共用prepare/asyncprecheck，is_subagent只影响scheduled-task reminder。

## Shell real entry verification

父亲自读取最终两份运行日志和fixture，shell-90090f680bca0698中4个exact均实际selected1、passed1、failed0、4587 filtered out：

| Exact suffix | Result |
| --- | --- |
| compaction_preserves_pending_model_skill_for_next_real_request | 1/1，0.32秒；本fixture old_selectedcount7，safe2，真实summary和下一普通request完整保留body/IDs/call/result，load一次、无terminal settle，canonical后清pin |
| compaction_without_safe_skill_prefix_never_requests_a_summary | 1/1，0.31秒；old7，safe0，summary provider零请求，Surface IDs不变 |
| cold_skill_materialization_projects_pending_and_consumed_ids | 1/1，0.02秒；encoded真实journal→validated冷物化投影pins，canonical admission后为空，原IDs前缀保持 |
| user_skill_remains_pinned_until_first_accepted_response_then_compacts | 1/1，0.15秒；原User guard与消费后恢复摘要资格保持 |

Rust/Cargo/rustdoc1.93.1，RUST_MIN_STACK16777216，独立GROW_HOME=/tmp/grow-skill-compaction-acceptance.du7lL6。最终skill_loading_tests blob4d7da80b3ffc36944486af257ff4f95188c73b5b，原8生产/调用方文件不变。父核对Observe通知作为load已发布证据，首provider末端阻塞时取得真实Step gate，释放toolcall后等待通知，compaction之后释放gate允许下一主请求；没有手置消费或伪造响应来解除保护。

中间失败保留：缺测试stack helper先导致编译失败；cold fixture空input user Turn违反既有契约，改为合法internal；PostToolUse实际是Observe通知而非ExtMethod gate，旧夹具等待错屏障超时，真实load/后继采样已先完成。均只修test，不修改生产或放松断言。最终32MiB局部栈helper、实际通知+既有Step gate解决；未把这些fixture失败误报为production或删场景。

独立luna静态review无生产阻塞，父已接受模块与实际入口证据。当前合并源码的默认CLI check/build均exit0，并以新CLI完成child model/effort/resume组合3/3和隔离Goal1/1。命令、binary SHA256和隔离范围见[同批集成记录](../2026-11-05-control-running-subagent-model-effort/verification.md#integrated-cli-acceptance)；这组进程测试补跨特性编译/调用链，compaction自身行为仍以本页planner、materialization、真实summary入口四exact为验收依据。

## Isolation

用户要求正常运行不受干扰。运行测试只使用独立GROW_HOME与loopback mock，自建测试进程自行回收；不安装、重启当前Grow或修改正常会话存储。

## Specification validation

归档前全量strict19/19；本change连同已验收的child sampling和Windows history修复归档合入主规范后，全量strict16/16、全历史archive853/853，0 failed。全workspace Rust1.93.1 Cargo fmt --check、git diff --check均通过。父按实际canonical result接受实现、静态review与真实入口续跑交付；没有以静态无阻塞替代行为测试。
