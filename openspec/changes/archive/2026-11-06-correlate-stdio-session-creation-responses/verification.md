# Verification

父核对实际bridge owner仍为StdioReplayState。pending creation按精确serde_json::Value requestID查找/移除，numeric与string身份区分；incoming方法消息/无id/不匹配id不会消费pending，error或无有效sessionId只结算匹配条目，成功后原cwd/MCP快照与assignedsessionId绑定。出向load/close及confirmed确定顺序保持；stdout接线也将带id的error交给同parser。

`cargo +1.93.1 test --locked --offline -p cli --bin grow -- cache_ replay_ forwarding_ concurrent_ numeric_and_string_request_ids failed_creation_and_unmatched duplicate_creation_response --test-threads=1` **21 passed / 0 failed、exit0**，日志 `/tmp/grow-backlog-pb02-cli-final.log`。原17CLIcontrols保持；四个新增覆盖反序双创建的完整forward→responsecache→reconnect replay、numeric/stringID独立结算、error与无关load交织后只replay成功session、重复响应不改已确认快照。父将双创建MCP夹具从无效object改为标准HTTP数组再编译运行；mock通道不实际连接该占位URL。

初筛命令仅跑18项（前三个新增名不匹配filter），父没有把它当全部通过，最终命令明确包含四项名字。numeric/string与duplicate的断言到cacheowner为止，其余实际replaycontrols覆盖确认发布/只加载正确集合；不声称所有单例都经重连。

本项未实现过载预算或非法重复inflightID政策；ACP容量债务仍按既定门槛单独评估。未调用真实leader、真实session或外部provider。
