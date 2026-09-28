## Context

实际请求经过 `sampler/src/client.rs::check_and_record_request` 保存 body、backend 和去敏 endpoint，Shell 的 `sampling_evidence` 将不可变 body 分为 64 KiB chunks 并在 Timeline Observation 中引用。请求 owner/source projection 可包含 Surface 身份、图片选择和 epoch。endpoint 证据会移除 query，因此仅有 endpoint 字符串不足以确认完整 route 等价。

现有 `assert_prefix_stable_pair` 比较 Responses 的 `input`，不包括 tools、输出 schema 和推理参数。Reasonix 的 `CaptureShape` 对 tools 排序后计算 hash，不能用来验证实际数组顺序。这些事实决定工具只能报告可见变化，不能恢复 provider 隐藏 prompt、tokenizer、TTL 或节点状态。

## Goals / Non-Goals

交付一个开发者可直接运行的离线 Python 工具，依赖标准库，处理完整快照中的既有格式。输出 JSON 摘要和可读摘要，解释哪些证据存在、哪些字段改变、usage 是否可比较。

不接入 Grow 产品 CLI/UI，不增加生产日志、后台服务或采样状态。该工具不发网络请求、不重放工具、不从证据恢复 native continuation。

## Decisions

### 1. 输入与边界

入口使用 `scripts/analyze_prompt_cache.py --session-dir <snapshot> --request <id:attempt> --baseline <id:attempt> --format json|text`。Sideband 使用其既有 `sideband_id/attempt_no` 归属，具体 identity 编码由脚本 help 说明。输入必须含 Timeline 及其 sampling chunks；先根据引用定位，不递归读取任意工作区内容。

按记录 bytes、chunk 大小和 digest 校验选中证据，保留既有最大 body 上限。逐条扫描索引，一次只加载选中请求对及对应有界响应，不把整个会话所有 body 常驻内存。拒绝越出快照目录的路径或符号链接引用。

选中证据损坏/缺块返回定位明确的非零错误；合法证据缺少可选元数据时正常输出 unavailable。脚本不修复或覆盖原文件，不给缺块请求伪造 body。先支持有完成边界的快照，不承诺对写入中的目录生成一致快照。

### 2. 比较归属由身份与因果证据决定

主请求只在同 session/branch、同可确认 route、同用途且有明确先后源关系时标为可比较。重试用 attempt 身份单独列出；Sideband 使用冻结来源及 purpose；并发旁路不能覆盖主请求基线。

用户显式选中跨 route、跨 branch 或主请求/Sideband 请求对时，可以展示差异，但比较状态必须标明对应边界，不能称其为同一路由纯追加。仅凭时间相邻、显示 model 名或相同去敏 URL 不建立等价；可用的 durable model-route 事实参与判定，仍无法确认租户/部署等边界时保留未知。默认不推断亲缘请求，减少隐式误归因。

### 3. 按协议位置报告差异

| 部分 | 输出 |
| --- | --- |
| 原始 body | 字节数、digest、backend |
| tools / system | 顺序和内容是否变化、首个变化位置 |
| messages / input | 结构化公共项边界、首个变化路径、是否仅追加 |
| 推理、输出 schema、tool choice 等设置 | 可见字段的增删改 |
| cache 参数 | key/retention/breakpoint 是否变化；key 默认只显示 digest |
| source projection | Surface 身份、图片选择、epoch 和 native span 边界变化 |
| usage / duration | 原始字段 presence、normalized 已知值及覆盖范围；缺失保留 unavailable |

分别保留 raw digest 与结构化差异。对象键序的格式差异可与内容差异分列，数组顺序必须保留；不先排序 tools 再宣称未改变。不将 JSON 长度或公共字节数除以常数作为 cached tokens。

输出将“观察到的请求差异”“provider 报告的用量”“证据不足”分列。已知变化仅是解释候选；即使所有可见字段稳定，也不能推导缓存可用或断言 miss 的服务端原因。现有 test 名称中的 prefix 只解释为 input/items prefix。

### 4. 输出与依赖

默认只输出结构路径、大小和 digest，不输出原始用户内容、图片 base64、工具参数或凭据；完整输入仍留在现有证据文件。JSON 的状态使用明确的 known/unavailable/not-comparable，避免由 0 或空字符串隐式编码未知。

工具读取与请求对比可以先落地；用量关联部分以 `preserve-cache-usage-availability` 的已完成格式核对，不自行建立与生产 mapper 不同的计费规则。旧证据没有可靠归属或分类时按可用范围展示，不回填历史账单。

## Risks / Trade-offs

- 原始 HTTP 前缀不等于模型 token 前缀：报告只命名结构化差异，不使用确定的“缓存失效原因”。
- snapshot 可能很大：一对一加载、长度上限和逐条索引；验证输出不包含大载荷。
- 归属不完整：宁可输出 not-comparable，也不让并发 Sideband 或重试污染主路径结论。

## Validation

合成 fixtures 覆盖纯追加、tools 重排/改 schema、effort 改变、图片淘汰、native reset、route 变化、并发 Sideband、重试与已知/未知 usage。损坏 digest、缺块和逃逸引用必须失败；同 fixtures 输出须确定且原文件 digest 不变。

计划执行 `python3 -m unittest discover -s scripts -p 'test_analyze_prompt_cache.py'`。对选定完整快照执行一次只读 smoke，核对请求 identity 与报告边界；无合适快照时保留未执行说明，不能把 fixture 测试称为真实 provider 命中验证。
