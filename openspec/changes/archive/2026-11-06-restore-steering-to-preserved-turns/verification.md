# Verification

父核对候选生产差异：`restore_pending_human_inputs`一次查询canonical runtime，只有preserves_open_turn且checkpoint.turn匹配才重建原actor PendingInterjection/ResidualInterjectionRequeue，包含原input_ids、prompt_id、全文、图片和requestmetadata；不appendRerouted、不入FIFO、不运行Hook/Submitted、不提前consume。非preserved仍走原validatedFIFO重路由。ChatState validator、runtime phase和releaseowner均未修改。

局部合法因果对照：主工作树联合命令 `cargo +1.93.1 test --locked --offline -p shell --lib -- mcp_server_validation session::acp_mcp::tests restore_steer_preserved_tests --test-threads=1` **9 passed / 0 failed、exit0**，日志 `/tmp/grow-backlog-mcp-input-second.log`，本项其中2条为preserved checkpoint重建原buffer（无新facts/FIFO）和无checkpoint interruption先关闭T再恰一次rerouteFIFO。原fixture的enum错误路径与checkpoint.step=None不符LoopStarted(0)已按实际类型和合法activeStep修正，不放宽validator。

物理冷恢复测试 `agent::mvp_agent::tests::steer_cold_restore_tests::preserved_steer_survives_cold_restore_and_consumes_once_after_release` 经两个独立OS进程：自有TempDir的HOME/GROW_HOME+同port本地mockprovider；第一阶段真实new/prompt推进到受控provider请求，真实grow/steer取得durableACK且仍AllowedSteer(T)，suspend_session_tree完成checkpoint和旧actor/persistence退休后退出；第二阶段真实load读取35条物理Timeline事实并经Prepare/Resumed/ExecutionReleased，原T安全边界消费同input一次，provider真实请求看见原全文，canonical消费项保留合法PNG。断言Submitted=1、input Hook=1、Consumed=1、Rerouted=0、原TurnID不变、Release先于Consumed，图片在canonical消费项中保留。

实际运行：`/tmp/grow-backlog-f1-cold-run4.log` **1 passed / 0 failed、exit0**。同产物 `/tmp/grow-backlog-f1-cold-run5-regress.log` **4 passed / 0 failed、exit0**：冷恢复、两个局部F1对照和一条MCP child policy reload control。后者不是MCP-F3七项，按真实名字更正旧报告分类，不重复计数。

父先后review并拒绝了未执行的fixture假设：prompt future未poll、provider未发arrival通知、shadowed input_id导致自比较、HOME未隔离、记录模型请求的竞争，以及缺少bool返回的predicate；均在独立测试owner中修正并实际运行。合法图片还需遵循现normalize要求：2×2不足边长、8×8不足512总像素，最终24×24 PNG通过，未改产品图像政策。run1编译红、run2/3非法图片红保留在/tmp对应日志；panic后的teardown错误不冒充产品根因。

只mock provider，session/actor/storage/挂起与恢复路径为生产实现；不连接真实用户目录、远端模型或leader，不声称复现用户Windows现场。F2 FIFO编辑位置/运行identity仍是独立项。
