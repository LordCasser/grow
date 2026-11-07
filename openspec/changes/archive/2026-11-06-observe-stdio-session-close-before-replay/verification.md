# Verification

父核对实际生产diff只有forwarder的一处调用：每个nonemptyline进入现cache parser，移除遗漏close的substring allowlist。方法身份与状态仍由既有helper决定，无新生命周期表、无close响应政策改变。

`cargo +1.93.1 test --locked --offline -p cli --bin grow -- cache_ replay_ forwarding_ --test-threads=1` **17 passed / 0 failed，真实exit0**，日志 `/tmp/grow-backlog-pb01-cli.log`。新测试走真实forward initialize/loadA/loadB/closeA→replacement reconnect replay，覆盖grow/session/close与_grow/session/close两种当前拼写；仅B收到load并成为primary，A不复活。无关prompt payload的生命周期文字不修改cache。既有replay错误/流通知/未确认new回归保持通过。

测试对端为本地channels的mockleader，不连接用户leader或真实模型；这验证了本项forward/cache/replay实际函数接线，不宣称整个daemon崩溃产品场景。PB-02并发new的RPC身份配对仍单独处理。PB-07共享main.rs的启动hunk未被此补丁覆盖。
