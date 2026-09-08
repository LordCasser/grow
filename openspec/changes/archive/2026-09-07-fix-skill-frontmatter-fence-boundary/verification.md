## 复现
旧实现 frontmatter_requires_complete_delimiter_lines 失败：---not-frontmatter 开头的整段被截为 Keep this body。循环首先在这一断言失败，不声称旧实现其余分支均已执行。

## 修复
extract_skill_body 与 parse_skill_frontmatter 共用 split_skill_frontmatter；逐行检查 trim 后完整 ---，保留当前流式读取的空白容忍规则。未闭合或前缀伪分隔符不解析为完整 frontmatter。未改变 YAML 恢复、大小上限或 body 前导空白处理。

## 验证
完整 Cargo tools lib implementations::skills：93 项全部通过（0.01s），包含前缀、LF、CRLF、EOF 及原有元数据、正文加载、参数和链接回归。
环境：CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216。命令 cargo test --locked --offline -p tools --lib implementations::skills --quiet。

没有将有效成对分隔线与 Markdown 水平线做语义区分；仍按既有 frontmatter 约定处理。未运行 UI 端到端测试。
