# 搜索S2当前成本（待架构结论，不代表已关闭）

临时诊断见[probe](search-cost-probe.rs)。当前canonical四event/turn（Started/User/Assistant/Ended）、assistant每turn约896B、Unicode prompt，无真实session、无provider/外部MCP。文件先写入并flush；每次创建pinned reader在计时外，2warmup+7samples，完整输出相等。debug warm文件，未控制OS缓存/CPU。reader decode包含逐record wire/schema/checksum/AppendState及完整JournalData读取；fold为Timeline::from_events；extract为现有selected-branch+prompt正文和上限处理。无SQLite写/worker排队/ACP/paint，非端到端。

命令：`cargo test --locked -p shell --lib backlog_search_cost_probe -- --ignored --nocapture --test-threads=1`，临时GROW_HOME和RUST_MIN_STACK=16777216。1 passed、4490 filtered、7.10s；此时其它cost probe也暂在编译，因此发现数不作为最终库数量。纯diagnostic，不改业务行为。


test session::storage::search::tests::backlog_search_cost_probe ... SEARCH_COST turns=128 bytes=305501 output=102196 samples_ns=[[24146625, 331791, 540583], [24447875, 298792, 556459], [24258750, 319375, 566666], [23628208, 290291, 531792], [23981667, 320708, 543375], [23869708, 290667, 531709], [23841250, 289375, 530917]]
SEARCH_COST turns=512 bytes=1226905 output=109108 samples_ns=[[97278416, 1277042, 2373333], [96697375, 1282833, 2352417], [95558583, 1220250, 2288583], [95508291, 1215208, 2302083], [95687375, 1258625, 2376000], [96328833, 1227083, 2279875], [95229000, 1218208, 2290958]]
SEARCH_COST turns=2048 bytes=4924203 output=137804 samples_ns=[[390553917, 5997333, 10452542], [388150667, 5483833, 10405875], [391464042, 5394750, 10406125], [386062917, 5333459, 10204250], [382008041, 5296625, 10096250], [385408250, 5369417, 10170459], [383458958, 5356250, 10238583]]
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4490 filtered out; finished in 7.10s

