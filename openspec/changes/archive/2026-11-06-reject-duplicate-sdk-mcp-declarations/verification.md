# Verification

父实读parser与new/load/spawn三个入口，重复校验共用同一pure parser，外层在任何resolve/load retirement/trust/persistence前拒绝，内部builder也在首个prepare副作用前解析并复用注册Vec。只按单SDKarray成功解析的canonicalname精确相等拒绝，标准array原校验及跨source precedence均未改；malformed条目/absentmeta原政策保留。

主工作树 `cargo +1.93.1 test --locked --offline -p shell --lib -- mcp_server_validation session::acp_mcp::tests restore_steer_preserved_tests --test-threads=1` **9 passed / 0 failed、actual exit0**，日志 `/tmp/grow-backlog-mcp-input-second.log`。本项为其中 **7项**：三条实际handler（标准update重复、SDK new重复、SDK resident load重复）和四条parsercontrols（valid+malformed、duplicateerror、malformedrepeat仍skip、absent）。另两项是F1输入恢复局部对照，不计入本项七项。原报告“7 validation + 4 parser + 2 F1”的数字有误，按实际输出更正。

新new/load测试直接调用真实ACP trait入口，验证duplicate-specific invalid_params、未发布会话/存储、resident旧handle/catalog保持及commandsEmpty。父review发现原测试调用initialize会启动与本项无关且未隔离的清理/search/coordinator任务，因此在首次执行前移除了initialize/auth设置；earlyguard本应先于initialize要求，漏guard会以不同错误失败specificassert。测试未运行真实用户session、provider或leader。需要的traitimport已由真实编译确认。

原补丁test末尾context多要一空行而不能apply，父只合入可干净apply的三个生产文件，再由单独owner按实际基准追加两条测试，不覆盖现有内容。联合测试的首次E0433和随后checkpointfrontier失败均在独立F1测试夹具，已合法修正后全9绿；没有放宽SDK或Timelinevalidator。

本项不实现MCP-F1 retired-client调用准入或MCP-F2 reverse路由，后两项仍在backlog；未声称任何新增SDK hot-update能力。
