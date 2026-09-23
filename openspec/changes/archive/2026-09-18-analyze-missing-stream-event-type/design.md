## Context

主规范区分远端服务失败、完整但无效的生成、确定性协议错误与缺少终止的流中断。当前 `Serialization` 不可重试。原始 SSE 记录足以判定截图中的 type 缺失来自哪一层，而 UI 错误本身不足以判断应否重试。

## Decisions

1. 以该 session 的 request/response/recovery_stop/usage settlement 为证据，原始 assistant 内容不写入仓库。
2. 主分析核对 client decoder → SamplingError → retry classifier → attempt admission；确定性证据提取委派给 Luna xhigh 只读子任务。
3. 使用合成 `request_id` 的最小错误帧与 loopback HTTP 200 SSE 复现真实客户端入口。探针只在本 change 保存，临时 Cargo example 入口运行后删除。
4. 只给出必要性和明确修复边界；行为实现应独立建立 change，保留当前共享预算、用量结算和工具接纳边界。

## Risks / Trade-offs

上游的内容检查只能证明端点报告了阻断，不能由该文本推断具体触发词、违规判断是否正确或模型推理内部原因。已流式返回的片段与已接纳工具副作用分开核对；缺少最终用量不能报告为零。
