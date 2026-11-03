# 验证

真实 LoadSession dispatch -> AppView input -> 导航 dispatch：原实现 swallowed 导航，后台失败回执从 peer 2 抢回 origin 0；两项旧行为均红。修复后普通/attached Ctrl+\、Ctrl+[、Ctrl+]、attached frame 返回点击通过，replay owner 和 composer 内容仍在原页。第一次绿测 fixture 的来源页无历史/标题，被既有空行隐藏规则排除，补真实可见标题后验证 peer cycle，没有修改该规则。

隔离 GROW_HOME、CI stack：replay_ 131 通过、3 ignored；session load 67、后台归属 1、dispatch dashboard 162 通过。既有文字/Enter/paste 冻结测试保持通过。

composition root `cargo build --locked -p cli --bin grow --no-default-features --features sandbox-enforce` 通过。日志 /tmp/grow-replay-nav-{red,green,debug,final,regressions}.txt；构建 /tmp/grow-journey-final-build.txt。新增组合 PTY 用例将单独记录其真实运行结果，不用单元测试冒充端到端。

OpenSpec 全量 strict：归档前 17、后 16；含归档 776 项通过。
