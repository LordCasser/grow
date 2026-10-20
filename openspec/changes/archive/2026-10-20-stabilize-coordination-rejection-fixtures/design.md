## Design
在现有 coordination tests 模块增加一个有界 fixture helper，启动既有 MockInferenceServer 并为 /v1/chat/completions 排队一次 HTTP 400。将 test actor 的 sampling route 指向该 server，返回持有 server 生命周期的句柄。两个失败测试均断言失败状态及 server 只收到一个请求，并保留各自原有业务断言；不把跨平台连接拒绝耗时当成业务证据。

CI 原始 RED：Windows job 118625552333，56 passed、2 failed；失败名 inquiry_uses_one_info_request_sideband_without_mutating_parent_surface、cross_workspace_approval_offers_only_one_shot_choices，均为 5 秒 Elapsed。修复后运行本地两用例/coordination组，并在正式同 SHA 三平台门禁重验。
