# MD-1 bounded measurement preparation

本记录只保存父核现状与待测边界，不表示性能收益已测或输出表示已决定。

父已读markdown/open_fence_highlighter.rs、syntax.rs和现criterion bench：HlLine=Vec<(SyntectStyle,String)>，open committed_lines和closed memo都持完整styled strings；highlight_block返回拥有的Vec。open cache hit在committed_len==text.len时clone全部committed_lines，追加分支也clone已完成prefix；closed命中同样clone。syntect parse已增量化，因此源码只证明每帧copy随已有prefix增长，不能把当前所有render成本归给parse。调用方最终需要Ratatui输出，借用缓存若在下层又完整clone一次就没有解决总代价。

先用固定theme、同language、逐行增长的代表性长fence测三项：冷parse、warm无新增的cache copy、append-only parse+copy；同时测StreamingMarkdownRenderer完整调用链与末尾batch输出一致。记录body bytes、lines/spans、frame分位耗时和累计复制/分配，控制热身/采样；不要用debug wall time直接当生产帧预算。现benches/bench.rs可复用，但已闭合短block混合文档不能代表长期开放fence。新增性能probe归独立change目录验证，临时原始日志留/tmp，不把巨大逐次数据提交。

测量后再决定最小所有权边界：可能复用已格式化行或改变tail快照，但必须检查parse→output→streaming consumer全链，不能只把String换Arc然后又在Span<'static,Cow<str>>重建。保style/width/theme reset、非append编辑、unknown syntax fallback、partial line临时parse state，以及闭合memo256KiB预算语义。性能修复与ANSI/跨style字素延期项无关，不混入。

按真实收益决定需要哪一表示变化；不为关闭backlog而声称copy复杂度已消除，也不在没有测量前搭共享render框架。若测量只能支持特定范围结论，记录范围、当前ceiling和有依据的触发条件。行为或public输出所有权变更前另change/delta，现记录不授权扩张接口。
