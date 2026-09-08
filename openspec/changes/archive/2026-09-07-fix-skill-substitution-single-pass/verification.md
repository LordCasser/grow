## 复现
旧实现 substitutions_preserve_inserted_token_text 失败：参数字面 ${SESSION_ID} 经 $ARGUMENTS 插入后变成 real-session。

## 验证
完整 Cargo tools lib implementations::skills::skill::tests：50 项通过。包含参数嵌套 token、目录嵌套 token、显式大索引空值、既有金额/后缀/插件路径/混合变量，以及内容加载和格式化回归。新回归覆盖四种输入，旧实现首先在字面 session token 断言失败，未声称四种分别运行过旧失败。

命令环境：CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p tools --lib implementations::skills::skill::tests --quiet。

没有更改 shell 引号解析（参数仍 whitespace-split），没有更改 $N 既有金额识别范围。未运行 UI 端到端测试。
