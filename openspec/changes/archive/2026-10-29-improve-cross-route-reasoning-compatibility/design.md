## Context
中性 Timeline 与当前路由的 ephemeral native continuation 已分离。`SamplingErrorInfo.portable_reasoning_required`、ChatState acknowledged enable、ContinuationLane 及 bounded resubmit 是现有完整路径。本次补齐它的输入证据与 Responses 投影范围。

## Decisions
1. 在现有 SamplingError::Api 上保存 API 专属 typed reasoning requirement；HTTP 从完整结构化 error code/param/message 分类后再生成展示文案，SSE 复用相同判定。DTO 重建直接保留 typed 值，不从截短 UI 字符串重新猜测。无结构化错误不扫描 HTML/普通正文。
2. 保持 HTTP 400 和唯一准确字段边界。既有 thinking/pass-back 要求继续有效；结构化缺失参数及明确的该字段 required 表述可作为直接证据。字段冲突、unsupported/invalid 值、泛化缺失字段、不同 backend 与 401/403/5xx 不授予 reasoning 修复。保留目标 backend 的最终 ACK 校验。
3. 扩大已学习 Responses 的 portable reasoning 至普通非空 assistant 前的既有可见文本；不能跨 User/System/ToolResult/其他 assistant 绑定，不能生成孤立 reasoning 或吞并无关 native span。默认路由和其他 backend 不改变。token 估算继续复用同一 projector。
4. Responses 无可回放文本时不能声称改变了请求；Chat 沿用空 reasoning_content 的既有行为。可见摘要不等于完整原生思考，本阶段不改变持久化 schema，真实验证必须明确成功或限制。
5. 使用隔离 Grow home、工作目录与本次编译产物，经 ACP 对同一 resident session 进行真实路由切换。只复制所需本地 provider 配置到私有临时文件，不输出凭据。业务工具使用可核对次数的本地夹具；观察实际请求路由、投影、恢复与工具执行，不通过伪造模型回复或自动结束背景等待制造通过。

## Validation
定向回归覆盖长 HTTP/SSE 错误、结构化 param/code、冲突与否定输入、typed DTO roundtrip、普通回答/无文本/边界与重复恢复。真实矩阵尽量覆盖三协议六方向及同协议换 provider/model、A→B→A；实际路由可用性和未验证格子如实记录，不以更换目标掩盖失败。复用一次 build，编译放专用临时 target 并在结束后清理。
